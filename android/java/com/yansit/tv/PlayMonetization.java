package com.yansit.tv;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;

/**
 * Real monetization layer (Gradle build only): Play Billing 8 + AdMob interstitial.
 * Instantiated by name from {@link Monetization#create}. GPL-3.0-or-later.
 */
final class PlayMonetization extends Monetization {
    private Activity activity;
    private BillingManager billing;
    private AdsManager ads;

    @Override
    void onCreate(Activity activity) {
        this.activity = activity;
        ads = new AdsManager(activity);
        billing = new BillingManager(activity, new Runnable() {
            @Override
            public void run() {
                // Never request/load ads for a paying user, nor before ownership is known.
                ads.setAllowed(billing.adsAllowed());
            }
        });
        ads.start();
        billing.start();
    }

    @Override
    void onResume() {
        billing.onResume();
    }

    @Override
    void onDestroy() {
        billing.destroy();
        ads.destroy();
        activity = null;
    }

    @Override
    void onBillingAction(int action) {
        switch (action) {
            case 1: billing.buy(true); break;
            case 2: billing.buy(false); break;
            case 3: billing.restore(); break;
            case 4: billing.manageSubscriptions(); break;
            case 5: openReview(); break;
            default: break;
        }
    }

    private void openReview() {
        if (activity == null) return;
        try {
            Intent intent = new Intent(Intent.ACTION_VIEW, Uri.parse("market://details?id=com.yansit.tv"));
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            activity.startActivity(intent);
        } catch (Throwable t) {
            try {
                Intent webIntent = new Intent(Intent.ACTION_VIEW, Uri.parse("https://play.google.com/store/apps/details?id=com.yansit.tv"));
                webIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                activity.startActivity(webIntent);
            } catch (Throwable ignored) {}
        }
    }

    @Override
    void showInterstitial() {
        // Rust already gates on free tier + between sessions; re-check Pro here as a backstop.
        if (billing.adsAllowed()) ads.show();
    }
}
