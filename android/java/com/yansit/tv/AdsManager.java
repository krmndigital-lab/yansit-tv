package com.yansit.tv;

import android.app.Activity;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import com.google.android.gms.ads.AdError;
import com.google.android.gms.ads.AdRequest;
import com.google.android.gms.ads.FullScreenContentCallback;
import com.google.android.gms.ads.LoadAdError;
import com.google.android.gms.ads.MobileAds;
import com.google.android.gms.ads.RequestConfiguration;
import com.google.android.gms.ads.interstitial.InterstitialAd;
import com.google.android.gms.ads.interstitial.InterstitialAdLoadCallback;

import java.util.Arrays;

/**
 * Google Mobile Ads (AdMob), interstitial only, free tier only (docs/MONETIZATION.md).
 *
 * <p>Android TV ad policy: an interstitial is a full-screen, dismissible ad in its own activity,
 * never an overlay. It is shown only when Rust asks via N.pollShowInterstitial(), which Rust does
 * only between mirror sessions (never over live video) and only on the free tier.
 *
 * <p>Production AdMob ids (the app id lives in AndroidManifest.xml,
 * com.google.android.gms.ads.APPLICATION_ID). Test devices are never hard-coded: debug builds read
 * them from android/local.properties (admob.testDevices=HASH1,HASH2, gitignored) via BuildConfig;
 * release builds have none (register the owner's TV in AdMob > Settings > Test devices instead).
 * GPL-3.0-or-later.
 */
final class AdsManager {
    /** Master switch ("ads_enabled"). false = the Ads SDK is never initialised, loaded or shown. */
    static final boolean ADS_ENABLED = true;

    /** KRMN Digital production AdMob interstitial unit. */
    static final String INTERSTITIAL_UNIT_ID = "ca-app-pub-9385203501275935/3742212452";

    private static final String TAG = "yansit-ads";
    private static final long RETRY_MIN_MS = 30_000L;
    private static final long RETRY_MAX_MS = 15 * 60_000L;

    private final Activity activity;
    private final Handler main = new Handler(Looper.getMainLooper());
    private InterstitialAd interstitial;
    private boolean initialized;
    private boolean allowed;
    private boolean loading;
    private boolean showing;
    private boolean destroyed;
    private long retryMs = RETRY_MIN_MS;

    private final Runnable retry = new Runnable() {
        @Override
        public void run() {
            load();
        }
    };

    AdsManager(Activity activity) {
        this.activity = activity;
    }

    void start() {
        N.adLoaded(false);
        if (!ADS_ENABLED) return;
        String testDevices = BuildConfig.ADMOB_TEST_DEVICES;
        if (!testDevices.isEmpty()) {
            MobileAds.setRequestConfiguration(new RequestConfiguration.Builder()
                    .setTestDeviceIds(Arrays.asList(testDevices.split(",")))
                    .build());
        }
        // GMA SDK 24+: initialise off the main thread (it does disk/network work; avoids ANRs).
        final android.content.Context app = activity.getApplicationContext();
        new Thread(() -> MobileAds.initialize(app, status -> main.post(() -> {
            initialized = true;
            load();
        })), "yansit-ads-init").start();
    }

    /** Billing says whether ads may be used (confirmed free tier). false drops any loaded ad. */
    void setAllowed(boolean allowed) {
        if (this.allowed == allowed) return;
        this.allowed = allowed;
        if (allowed) {
            load();
        } else {
            main.removeCallbacks(retry);
            if (interstitial != null) {
                interstitial = null;
                N.adLoaded(false);
            }
        }
    }

    private void load() {
        if (!ADS_ENABLED || destroyed || !initialized || !allowed || loading || interstitial != null) return;
        loading = true;
        InterstitialAd.load(activity, INTERSTITIAL_UNIT_ID, new AdRequest.Builder().build(),
                new InterstitialAdLoadCallback() {
                    @Override
                    public void onAdLoaded(InterstitialAd ad) {
                        loading = false;
                        if (destroyed || !allowed) return;
                        interstitial = ad;
                        retryMs = RETRY_MIN_MS;
                        N.adLoaded(true);
                    }

                    @Override
                    public void onAdFailedToLoad(LoadAdError error) {
                        loading = false;
                        interstitial = null;
                        N.adLoaded(false);
                        Log.w(TAG, "Interstitial failed to load: " + error.getMessage());
                        if (destroyed) return;
                        main.removeCallbacks(retry);
                        main.postDelayed(retry, retryMs);
                        retryMs = Math.min(retryMs * 2, RETRY_MAX_MS);
                    }
                });
    }

    /** Shows the loaded interstitial (main thread); a fresh one is loaded once it closes. */
    void show() {
        if (!ADS_ENABLED || destroyed || !allowed || showing || interstitial == null) return;
        InterstitialAd ad = interstitial;
        interstitial = null;
        N.adLoaded(false); // consumed: Rust must not ask again until the next one has loaded
        ad.setFullScreenContentCallback(new FullScreenContentCallback() {
            @Override
            public void onAdDismissedFullScreenContent() {
                showing = false;
                load();
            }

            @Override
            public void onAdFailedToShowFullScreenContent(AdError error) {
                showing = false;
                Log.w(TAG, "Interstitial failed to show: " + error.getMessage());
                load();
            }
        });
        showing = true;
        ad.show(activity);
    }

    void destroy() {
        destroyed = true;
        main.removeCallbacksAndMessages(null);
        interstitial = null;
    }
}
