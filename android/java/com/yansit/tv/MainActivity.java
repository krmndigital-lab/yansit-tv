package com.yansit.tv;

import android.app.Activity;
import android.content.Intent;
import android.graphics.Color;
import android.graphics.PixelFormat;
import android.net.wifi.WifiManager;
import android.os.Bundle;
import android.view.Choreographer;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.WindowManager;
import android.widget.FrameLayout;

/**
 * Dual-SurfaceView Android TV host for Ayna:
 * - Lower SurfaceView (kind 0): bound directly to Amlogic hardware MediaCodec (HEVC / AVC).
 * - Upper SurfaceView (kind 1): translucent GLES2/3 surface where Rust draws the UI & HUD.
 */
public class MainActivity extends Activity {
    private WifiManager.WifiLock wifiLock;
    private long period = 16_666_667L;
    private boolean resumed;
    private FrameLayout root;
    private SurfaceView video;
    // Play Billing + AdMob (no-op in Gradle-free builds); see docs/MONETIZATION.md.
    private Monetization monetization;

    private final Choreographer.FrameCallback vsync = new Choreographer.FrameCallback() {
        @Override
        public void doFrame(long t) {
            if (!resumed) return;
            if (N.vsync(t, period)) {
                finish();
                return;
            }
            if (root != null) {
                long l = N.videoLayout(root.getWidth(), root.getHeight());
                if (l >= 0) {
                    int w = (int) (l >> 32);
                    int h = (int) l;
                    video.setLayoutParams(new FrameLayout.LayoutParams(w, h, Gravity.CENTER));
                }
            }
            int awake = N.awake();
            if (awake == 1) {
                getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
            } else if (awake == 0) {
                getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
            }
            // Monetization requests from the Rust UI / free-tier session gate (UI thread).
            int billingAction = N.pollBillingAction();
            if (billingAction != 0) monetization.onBillingAction(billingAction);
            if (N.pollShowInterstitial()) monetization.showInterstitial();
            Choreographer.getInstance().postFrameCallback(this);
        }
    };

    private SurfaceHolder.Callback surfaceCallback(final int kind) {
        return new SurfaceHolder.Callback() {
            @Override
            public void surfaceCreated(SurfaceHolder h) {
                N.surface(kind, h.getSurface());
            }

            @Override
            public void surfaceChanged(SurfaceHolder h, int f, int w, int hh) {
                if (kind == 1) N.resized();
            }

            @Override
            public void surfaceDestroyed(SurfaceHolder h) {
                N.surface(kind, null);
            }
        };
    }

    @Override
    protected void onCreate(Bundle b) {
        super.onCreate(b);
        float hz = getWindowManager().getDefaultDisplay().getRefreshRate();
        if (hz > 1f) period = (long) (1e9 / hz);

        String files = getFilesDir().getAbsolutePath();
        String cache = getCacheDir().getAbsolutePath();
        N.create(files, cache);

        // Ensure background foreground service + mDNS advertisement is running
        try {
            Intent svc = new Intent(this, AirPlayService.class);
            startForegroundService(svc);
        } catch (Throwable ignored) {
        }

        WifiManager wm = (WifiManager) getApplicationContext().getSystemService(WIFI_SERVICE);
        if (wm != null) {
            wifiLock = wm.createWifiLock(WifiManager.WIFI_MODE_FULL_LOW_LATENCY, "yansit:wifi");
            wifiLock.setReferenceCounted(false);
        }

        root = new FrameLayout(this);
        root.setBackgroundColor(Color.BLACK);

        video = new SurfaceView(this);
        video.getHolder().addCallback(surfaceCallback(0));

        SurfaceView overlay = new SurfaceView(this);
        overlay.setZOrderMediaOverlay(true);
        overlay.getHolder().setFormat(PixelFormat.TRANSLUCENT);
        overlay.getHolder().addCallback(surfaceCallback(1));

        root.addView(video, new FrameLayout.LayoutParams(-1, -1, Gravity.CENTER));
        root.addView(overlay, new FrameLayout.LayoutParams(-1, -1));
        setContentView(root);

        monetization = Monetization.create(this);
        requestMiracastPermission();
    }

    /**
     * Miracast (Wi-Fi Direct) peer discovery needs a runtime permission: NEARBY_WIFI_DEVICES on
     * Android 13+, fine location before that. AirPlay and DLNA work without it. Asked while
     * the app is open; Android stops showing the dialog once the user declines for good.
     */
    private void requestMiracastPermission() {
        final String perm = android.os.Build.VERSION.SDK_INT >= 33
                ? "android.permission.NEARBY_WIFI_DEVICES"
                : android.Manifest.permission.ACCESS_FINE_LOCATION;
        if (checkSelfPermission(perm) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            new android.app.AlertDialog.Builder(this, android.R.style.Theme_DeviceDefault_Dialog_Alert)
                    .setTitle(R.string.perm_dialog_title)
                    .setMessage(R.string.perm_dialog_desc)
                    .setPositiveButton(R.string.perm_dialog_ok, (dialog, which) -> {
                        requestPermissions(new String[] { perm }, 7);
                    })
                    .setNegativeButton(R.string.perm_dialog_later, null)
                    .show();
        }
    }

    @Override
    public boolean dispatchKeyEvent(KeyEvent e) {
        int a = e.getAction();
        if ((a == KeyEvent.ACTION_DOWN || a == KeyEvent.ACTION_UP)
                && N.key(e.getKeyCode(), a, e.getRepeatCount(), e.isLongPress(), e.getUnicodeChar(e.getMetaState()))) {
            return true;
        }
        return super.dispatchKeyEvent(e);
    }

    @Override
    protected void onStart() {
        super.onStart();
        N.life(0);
    }

    @Override
    protected void onResume() {
        super.onResume();
        resumed = true;
        if (wifiLock != null) wifiLock.acquire();
        N.life(1);
        monetization.onResume();
        Choreographer.getInstance().postFrameCallback(vsync);
    }

    @Override
    protected void onPause() {
        resumed = false;
        Choreographer.getInstance().removeFrameCallback(vsync);
        if (wifiLock != null && wifiLock.isHeld()) wifiLock.release();
        N.life(2);
        super.onPause();
    }

    @Override
    protected void onStop() {
        N.life(3);
        super.onStop();
    }

    @Override
    protected void onDestroy() {
        N.life(4);
        monetization.onDestroy();
        super.onDestroy();
    }
}
