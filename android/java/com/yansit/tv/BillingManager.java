package com.yansit.tv;

import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import com.android.billingclient.api.AcknowledgePurchaseParams;
import com.android.billingclient.api.BillingClient;
import com.android.billingclient.api.BillingClientStateListener;
import com.android.billingclient.api.BillingFlowParams;
import com.android.billingclient.api.BillingResult;
import com.android.billingclient.api.PendingPurchasesParams;
import com.android.billingclient.api.ProductDetails;
import com.android.billingclient.api.Purchase;
import com.android.billingclient.api.PurchasesUpdatedListener;
import com.android.billingclient.api.QueryProductDetailsParams;
import com.android.billingclient.api.QueryPurchasesParams;

import java.util.ArrayList;
import java.util.Collections;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/**
 * Google Play Billing Library 8 for Ayna Pro (docs/MONETIZATION.md):
 * <ul>
 *   <li>{@code yansit_pro_yearly}: subscription, base plan {@code yearly}</li>
 *   <li>{@code yansit_pro_lifetime}: one-time INAPP, non-consumable (acknowledged, never consumed)</li>
 * </ul>
 * Pro = active yearly subscription OR owned lifetime product. State goes to Rust through
 * {@link N#billingState}. Billing may call back on a background thread, so every callback hops to
 * the main thread and all fields are touched there only.
 *
 * <p>Safety rule: {@code pro=false} is never pushed until BOTH the SUBS and the INAPP ownership
 * queries have succeeded once. Rust caches Pro across cold starts, and a slow or failed Play
 * connection must not wipe a paying user's entitlement. GPL-3.0-or-later.
 */
final class BillingManager implements PurchasesUpdatedListener, BillingClientStateListener {
    static final String YEARLY = "yansit_pro_yearly";
    static final String YEARLY_BASE_PLAN = "yearly";
    static final String LIFETIME = "yansit_pro_lifetime";

    private static final String TAG = "yansit-billing";
    private static final long RETRY_MIN_MS = 2_000L;
    private static final long RETRY_MAX_MS = 5 * 60_000L;

    private final Activity activity;
    private final Runnable onChange;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final BillingClient client;

    private ProductDetails yearlyDetails;
    private ProductDetails lifetimeDetails;
    private String yearlyPrice = "";
    private String lifetimePrice = "";
    private boolean productsQueried;
    private boolean yearlyOwned;
    private boolean lifetimeOwned;
    private boolean subsKnown;
    private boolean inappKnown;
    /** Billing is permanently unusable here (no Play Store / unsupported): treat as free tier. */
    private boolean unavailable;
    private boolean connecting;
    private boolean destroyed;
    private long retryMs = RETRY_MIN_MS;
    /** Purchase tokens with an acknowledge call in flight (avoids duplicate acks on re-query). */
    private final Set<String> acking = new HashSet<>();

    private final Runnable reconnect = new Runnable() {
        @Override
        public void run() {
            connect();
        }
    };

    BillingManager(Activity activity, Runnable onChange) {
        this.activity = activity;
        this.onChange = onChange;
        this.client = BillingClient.newBuilder(activity.getApplicationContext())
                .setListener(this)
                // Billing 7+: pending one-time purchases (e.g. cash) must be opted in explicitly.
                .enablePendingPurchases(PendingPurchasesParams.newBuilder().enableOneTimeProducts().build())
                .build();
    }

    void start() {
        connect();
    }

    /** Re-check ownership on every resume (purchases/refunds/renewals made elsewhere). */
    void onResume() {
        if (destroyed) return;
        if (client.isReady()) {
            refreshPurchases();
            if (!productsQueried) queryProducts();
        } else {
            connect();
        }
    }

    void destroy() {
        destroyed = true;
        main.removeCallbacksAndMessages(null);
        try {
            client.endConnection();
        } catch (Throwable ignored) {
        }
    }

    boolean isPro() {
        return yearlyOwned || lifetimeOwned;
    }

    /** Ads only for a confirmed free user (or where Play Billing can't work at all). */
    boolean adsAllowed() {
        return !isPro() && ((subsKnown && inappKnown) || unavailable);
    }

    // ---------------------------------------------------------------- connection

    private void connect() {
        if (destroyed || connecting || client.isReady()) return;
        connecting = true;
        try {
            client.startConnection(this);
        } catch (Throwable t) {
            connecting = false;
            Log.w(TAG, "startConnection failed", t);
            scheduleReconnect();
        }
    }

    @Override
    public void onBillingSetupFinished(final BillingResult result) {
        main.post(new Runnable() {
            @Override
            public void run() {
                connecting = false;
                if (destroyed) return;
                int code = result.getResponseCode();
                if (code == BillingClient.BillingResponseCode.OK) {
                    retryMs = RETRY_MIN_MS;
                    unavailable = false;
                    queryProducts();
                    refreshPurchases();
                } else if (code == BillingClient.BillingResponseCode.BILLING_UNAVAILABLE
                        || code == BillingClient.BillingResponseCode.FEATURE_NOT_SUPPORTED) {
                    // No usable Play Store (e.g. some AOSP TV boxes). Keep Rust's cached Pro flag
                    // untouched (no push); only allow free-tier ads. onResume() retries.
                    Log.w(TAG, "Billing unavailable: " + result.getDebugMessage());
                    unavailable = true;
                    onChange.run();
                } else {
                    Log.w(TAG, "Billing setup failed (" + code + "): " + result.getDebugMessage());
                    scheduleReconnect();
                }
            }
        });
    }

    @Override
    public void onBillingServiceDisconnected() {
        main.post(new Runnable() {
            @Override
            public void run() {
                connecting = false;
                scheduleReconnect();
            }
        });
    }

    private void scheduleReconnect() {
        if (destroyed) return;
        main.removeCallbacks(reconnect);
        main.postDelayed(reconnect, retryMs);
        retryMs = Math.min(retryMs * 2, RETRY_MAX_MS);
    }

    // ---------------------------------------------------------------- products + prices

    private void queryProducts() {
        List<QueryProductDetailsParams.Product> subs = Collections.singletonList(
                QueryProductDetailsParams.Product.newBuilder()
                        .setProductId(YEARLY)
                        .setProductType(BillingClient.ProductType.SUBS)
                        .build());
        List<QueryProductDetailsParams.Product> inapp = Collections.singletonList(
                QueryProductDetailsParams.Product.newBuilder()
                        .setProductId(LIFETIME)
                        .setProductType(BillingClient.ProductType.INAPP)
                        .build());
        // One product type per query (mixing SUBS and INAPP in one request is rejected).
        queryProductList(subs);
        queryProductList(inapp);
    }

    private void queryProductList(List<QueryProductDetailsParams.Product> products) {
        client.queryProductDetailsAsync(
                QueryProductDetailsParams.newBuilder().setProductList(products).build(),
                (result, detailsResult) -> {
                    List<ProductDetails> details = detailsResult != null ? detailsResult.getProductDetailsList() : null;
                    main.post(() -> onProductDetails(result, details));
                });
    }

    private void onProductDetails(BillingResult result, List<ProductDetails> details) {
        if (destroyed) return;
        if (result.getResponseCode() != BillingClient.BillingResponseCode.OK || details == null) {
            Log.w(TAG, "Product query failed: " + result.getDebugMessage());
            return; // retried on next onResume()
        }
        for (ProductDetails pd : details) {
            if (YEARLY.equals(pd.getProductId())) {
                yearlyDetails = pd;
                yearlyPrice = yearlyRecurringPrice(pd);
            } else if (LIFETIME.equals(pd.getProductId())) {
                lifetimeDetails = pd;
                ProductDetails.OneTimePurchaseOfferDetails o = pd.getOneTimePurchaseOfferDetails();
                lifetimePrice = o != null ? o.getFormattedPrice() : "";
            }
        }
        productsQueried = yearlyDetails != null && lifetimeDetails != null;
        push();
    }

    /** Offer of base plan "yearly" (the plain base-plan offer preferred over promo offers). */
    private static ProductDetails.SubscriptionOfferDetails yearlyOffer(ProductDetails pd) {
        List<ProductDetails.SubscriptionOfferDetails> offers = pd.getSubscriptionOfferDetails();
        if (offers == null || offers.isEmpty()) return null;
        ProductDetails.SubscriptionOfferDetails any = null;
        for (ProductDetails.SubscriptionOfferDetails o : offers) {
            if (!YEARLY_BASE_PLAN.equals(o.getBasePlanId())) continue;
            if (o.getOfferId() == null) return o;
            if (any == null) any = o;
        }
        return any;
    }

    private static String yearlyRecurringPrice(ProductDetails pd) {
        ProductDetails.SubscriptionOfferDetails o = yearlyOffer(pd);
        if (o == null) return "";
        List<ProductDetails.PricingPhase> phases = o.getPricingPhases().getPricingPhaseList();
        // The last phase is the recurring price (earlier ones would be trial/intro phases).
        return phases.isEmpty() ? "" : phases.get(phases.size() - 1).getFormattedPrice();
    }

    // ---------------------------------------------------------------- purchases

    private void refreshPurchases() {
        client.queryPurchasesAsync(
                QueryPurchasesParams.newBuilder().setProductType(BillingClient.ProductType.SUBS).build(),
                (result, purchases) -> main.post(() -> onOwned(true, result, purchases)));
        client.queryPurchasesAsync(
                QueryPurchasesParams.newBuilder().setProductType(BillingClient.ProductType.INAPP).build(),
                (result, purchases) -> main.post(() -> onOwned(false, result, purchases)));
    }

    /** Authoritative ownership for one product type (queryPurchasesAsync = active/owned only). */
    private void onOwned(boolean subs, BillingResult result, List<Purchase> purchases) {
        if (destroyed) return;
        if (result.getResponseCode() != BillingClient.BillingResponseCode.OK || purchases == null) {
            Log.w(TAG, (subs ? "SUBS" : "INAPP") + " purchase query failed: " + result.getDebugMessage());
            return; // keep the last known value; never revoke on a transient error
        }
        boolean owned = false;
        for (Purchase p : purchases) {
            if (handlePurchase(p, subs ? YEARLY : LIFETIME)) owned = true;
        }
        if (subs) {
            yearlyOwned = owned;
            subsKnown = true;
        } else {
            lifetimeOwned = owned;
            inappKnown = true;
        }
        push();
    }

    /** Acknowledges if needed; returns true if {@code p} is a completed purchase of {@code productId}. */
    private boolean handlePurchase(Purchase p, String productId) {
        if (!p.getProducts().contains(productId)) return false;
        if (p.getPurchaseState() != Purchase.PurchaseState.PURCHASED) return false; // PENDING: not yet
        if (!p.isAcknowledged()) acknowledge(p);
        return true;
    }

    /** Both products are entitlements, so both are acknowledged (unacknowledged = refunded after 3 days). */
    private void acknowledge(Purchase p) {
        final String token = p.getPurchaseToken();
        if (!acking.add(token)) return;
        client.acknowledgePurchase(
                AcknowledgePurchaseParams.newBuilder().setPurchaseToken(token).build(),
                result -> main.post(() -> {
                    acking.remove(token);
                    if (result.getResponseCode() != BillingClient.BillingResponseCode.OK) {
                        // Retried by the next refreshPurchases() (onResume / restore).
                        Log.w(TAG, "acknowledge failed: " + result.getDebugMessage());
                    }
                }));
    }

    @Override
    public void onPurchasesUpdated(final BillingResult result, final List<Purchase> purchases) {
        main.post(() -> {
            if (destroyed) return;
            int code = result.getResponseCode();
            if (code == BillingClient.BillingResponseCode.OK && purchases != null) {
                for (Purchase p : purchases) {
                    if (handlePurchase(p, YEARLY)) yearlyOwned = true;
                    if (handlePurchase(p, LIFETIME)) lifetimeOwned = true;
                }
                push();
                refreshPurchases(); // re-sync the authoritative view
            } else if (code == BillingClient.BillingResponseCode.ITEM_ALREADY_OWNED) {
                refreshPurchases();
            } else if (code != BillingClient.BillingResponseCode.USER_CANCELED) {
                Log.w(TAG, "Purchase failed (" + code + "): " + result.getDebugMessage());
            }
        });
    }

    // ---------------------------------------------------------------- user actions (main thread)

    /** Launches the Play purchase sheet for the yearly subscription or the lifetime product. */
    void buy(boolean yearly) {
        ProductDetails pd = yearly ? yearlyDetails : lifetimeDetails;
        if (!client.isReady() || pd == null) {
            Log.w(TAG, "buy(" + (yearly ? YEARLY : LIFETIME) + "): billing not ready yet");
            onResume(); // reconnect / re-query; Rust shows the not-ready state from billingState
            return;
        }
        BillingFlowParams.ProductDetailsParams.Builder item =
                BillingFlowParams.ProductDetailsParams.newBuilder().setProductDetails(pd);
        if (yearly) {
            ProductDetails.SubscriptionOfferDetails offer = yearlyOffer(pd);
            if (offer == null) {
                Log.w(TAG, "No offer for base plan '" + YEARLY_BASE_PLAN + "' (check Play Console)");
                return;
            }
            item.setOfferToken(offer.getOfferToken());
        }
        List<BillingFlowParams.ProductDetailsParams> list = new ArrayList<>();
        list.add(item.build());
        BillingResult r = client.launchBillingFlow(activity,
                BillingFlowParams.newBuilder().setProductDetailsParamsList(list).build());
        if (r.getResponseCode() != BillingClient.BillingResponseCode.OK) {
            Log.w(TAG, "launchBillingFlow failed (" + r.getResponseCode() + "): " + r.getDebugMessage());
        }
    }

    /** "Restore purchases": Play keeps them per account, so restoring = re-querying ownership. */
    void restore() {
        if (client.isReady()) {
            refreshPurchases();
        } else {
            connect();
        }
    }

    /** Opens Play's subscription management (the yearly sub page when owned). */
    void manageSubscriptions() {
        String url = "https://play.google.com/store/account/subscriptions";
        if (yearlyOwned) url += "?sku=" + YEARLY + "&package=" + activity.getPackageName();
        Intent i = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
        i.setPackage("com.android.vending");
        try {
            activity.startActivity(i);
        } catch (ActivityNotFoundException e) {
            try {
                activity.startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
            } catch (ActivityNotFoundException e2) {
                Log.w(TAG, "No app can open " + url);
            }
        }
    }

    // ---------------------------------------------------------------- Rust bridge

    private void push() {
        boolean pro = isPro();
        // pro=true is always safe; pro=false only once both ownership queries have answered.
        if (pro || (subsKnown && inappKnown)) {
            N.billingState(pro, yearlyOwned, lifetimeOwned, yearlyPrice, lifetimePrice,
                    client.isReady() && productsQueried);
        }
        onChange.run();
    }
}
