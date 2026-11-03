package com.yansit.tv;

import android.app.Activity;
import android.util.Log;

/**
 * Seam between MainActivity and the Play Billing + AdMob layer (see docs/MONETIZATION.md).
 *
 * The Gradle build compiles {@code PlayMonetization}, {@code BillingManager} and {@code AdsManager}
 * (they need the Billing / Mobile Ads AARs). The Gradle-free scripts (build_apk.sh,
 * build_bundle.sh) cannot link AARs, so they leave those three files out; {@link #create} then
 * falls back to this no-op base class and the app still runs as the plain free tier.
 * GPL-3.0-or-later.
 */
class Monetization {
    private static final String TAG = "yansit";

    static Monetization create(Activity activity) {
        Monetization m;
        try {
            m = (Monetization) Class.forName("com.yansit.tv.PlayMonetization")
                    .getDeclaredConstructor().newInstance();
        } catch (Throwable t) {
            Log.i(TAG, "Billing/Ads layer not built in (Gradle-free build): " + t);
            return new Monetization();
        }
        try {
            m.onCreate(activity);
        } catch (Throwable t) {
            Log.w(TAG, "Billing/Ads init failed; continuing without them", t);
            return new Monetization();
        }
        return m;
    }

    void onCreate(Activity activity) {}

    void onResume() {}

    void onDestroy() {}

    /** From N.pollBillingAction(): 1 buy yearly, 2 buy lifetime, 3 restore, 4 manage subs. */
    void onBillingAction(int action) {}

    /** N.pollShowInterstitial() fired (Rust: free tier, between mirror sessions only). */
    void showInterstitial() {}
}
