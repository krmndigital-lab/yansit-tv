package com.yansit.tv;

import android.view.Surface;

public final class N {
    static {
        System.loadLibrary("yansit");
    }

    public static native int ensureServer(String files);
    public static native int port();
    public static native String serverName();
    public static native String raopName();
    public static native String airplayTxt();
    public static native String raopTxt();
    public static native boolean pollConfigChanged();
    public static native boolean pollWakeRequest();

    public static native void create(String files, String cache);
    public static native void surface(int kind, Surface s);
    public static native void resized();
    public static native boolean key(int code, int action, int repeat, boolean longPress, int uni);
    public static native void life(int ev);
    public static native boolean vsync(long frameTimeNanos, long periodNanos);
    public static native int awake();
    public static native long videoLayout(int sw, int sh);

    // --- Monetization bridge (see docs/MONETIZATION.md) ---
    // Java -> Rust: push billing state whenever it changes.
    public static native void billingState(boolean pro, boolean yearlyOwned, boolean lifetimeOwned,
                                           String yearlyPrice, String lifetimePrice, boolean ready);
    // Java -> Rust: an interstitial finished loading (so Rust only requests when one is ready).
    public static native void adLoaded(boolean interstitialReady);
    // Rust -> Java: 0 none, 1 buy yearly, 2 buy lifetime, 3 restore, 4 manage subscriptions.
    public static native int pollBillingAction();
    // Rust -> Java: true once when a free-tier interstitial should be shown (between sessions).
    public static native boolean pollShowInterstitial();
}
