package com.yansit.tv;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.net.nsd.NsdManager;
import android.content.pm.ServiceInfo;
import android.net.nsd.NsdServiceInfo;
import android.os.Build;
import android.net.wifi.WifiManager;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.PowerManager;
import android.util.Log;

/**
 * Persistent foreground service that keeps the Rust AirPlay server running in the background,
 * holds a WifiManager.MulticastLock, registers `_airplay._tcp` and `_raop._tcp` via Android
 * NsdManager, and wakes/launches MainActivity when an iPad/iPhone connects.
 */
public class AirPlayService extends Service {
    private static final String TAG = "yansit";
    private static final String CH_ID = "yansit_airplay_channel";
    private static final int NOTIF_ID = 7001;

    private WifiManager.MulticastLock multicastLock;
    private WifiManager.WifiLock wifiLock;
    private PowerManager.WakeLock wakeLock;
    private NsdManager nsdManager;
    private NsdManager.RegistrationListener airplayReg;
    private NsdManager.RegistrationListener raopReg;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private boolean running;

    private final Runnable pollTask = new Runnable() {
        @Override
        public void run() {
            if (!running) return;
            try {
                if (N.pollConfigChanged()) {
                    Log.i(TAG, "Configuration updated in UI — re-registering mDNS services");
                    unregisterMdns();
                    registerMdns();
                }
                if (N.pollWakeRequest()) {
                    Log.i(TAG, "Incoming AirPlay session — bringing MainActivity to foreground");
                    wakeAndLaunchUi();
                }
            } catch (Throwable t) {
                Log.w(TAG, "pollTask error", t);
            }
            handler.postDelayed(this, 400);
        }
    };

    @Override
    public void onCreate() {
        super.onCreate();
        startForegroundWithNotification();
        acquireLocks();
        int port = N.ensureServer(getFilesDir().getAbsolutePath());
        Log.i(TAG, "AirPlayService started, Rust RTSP server port=" + port);
        nsdManager = (NsdManager) getSystemService(Context.NSD_SERVICE);
        registerMdns();
        running = true;
        handler.postDelayed(pollTask, 400);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        return START_STICKY;
    }

    @Override
    public void onDestroy() {
        running = false;
        handler.removeCallbacks(pollTask);
        unregisterMdns();
        releaseLocks();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private void startForegroundWithNotification() {
        NotificationManager nm = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
        if (nm != null) {
            NotificationChannel ch = new NotificationChannel(
                    CH_ID, getString(R.string.notif_channel_name), NotificationManager.IMPORTANCE_LOW);
            ch.setDescription(getString(R.string.notif_channel_desc));
            nm.createNotificationChannel(ch);
        }
        Intent openIntent = new Intent(this, MainActivity.class);
        openIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_SINGLE_TOP);
        PendingIntent pi = PendingIntent.getActivity(
                this, 0, openIntent, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);

        Notification notif = new Notification.Builder(this, CH_ID)
                .setContentTitle(getString(R.string.notif_title))
                .setContentText(getString(R.string.notif_text))
                .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
                .setContentIntent(pi)
                .setOngoing(true)
                .build();
        // Receiving casts from phones/PCs over the network = "connectedDevice" (no location,
        // no media-playback boot restriction on newer Android versions).
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIF_ID, notif, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE);
        } else {
            startForeground(NOTIF_ID, notif);
        }
    }

    private void acquireLocks() {
        try {
            WifiManager wm = (WifiManager) getApplicationContext().getSystemService(Context.WIFI_SERVICE);
            if (wm != null) {
                multicastLock = wm.createMulticastLock("yansit:mdns");
                multicastLock.setReferenceCounted(false);
                multicastLock.acquire();

                wifiLock = wm.createWifiLock(WifiManager.WIFI_MODE_FULL_LOW_LATENCY, "yansit:svc_wifi");
                wifiLock.setReferenceCounted(false);
                wifiLock.acquire();
            }
        } catch (Throwable t) {
            Log.w(TAG, "Wifi lock error", t);
        }
    }

    private void releaseLocks() {
        try {
            if (multicastLock != null && multicastLock.isHeld()) multicastLock.release();
            if (wifiLock != null && wifiLock.isHeld()) wifiLock.release();
            if (wakeLock != null && wakeLock.isHeld()) wakeLock.release();
        } catch (Throwable ignored) {
        }
    }

    private void registerMdns() {
        if (nsdManager == null) return;
        int port = N.port();
        String srvName = N.serverName();
        String raopName = N.raopName();

        try {
            NsdServiceInfo airplayInfo = new NsdServiceInfo();
            airplayInfo.setServiceName(srvName);
            airplayInfo.setServiceType("_airplay._tcp");
            airplayInfo.setPort(port);
            applyTxtRecords(airplayInfo, N.airplayTxt());

            airplayReg = new NsdManager.RegistrationListener() {
                @Override public void onRegistrationFailed(NsdServiceInfo s, int e) {
                    Log.w(TAG, "_airplay._tcp reg failed: " + e);
                }
                @Override public void onUnregistrationFailed(NsdServiceInfo s, int e) {}
                @Override public void onServiceRegistered(NsdServiceInfo s) {
                    Log.i(TAG, "_airplay._tcp registered as '" + s.getServiceName() + "' on port " + port);
                }
                @Override public void onServiceUnregistered(NsdServiceInfo s) {}
            };
            nsdManager.registerService(airplayInfo, NsdManager.PROTOCOL_DNS_SD, airplayReg);
        } catch (Throwable t) {
            Log.e(TAG, "Failed to register _airplay._tcp", t);
        }

        try {
            NsdServiceInfo raopInfo = new NsdServiceInfo();
            raopInfo.setServiceName(raopName);
            raopInfo.setServiceType("_raop._tcp");
            raopInfo.setPort(port);
            applyTxtRecords(raopInfo, N.raopTxt());

            raopReg = new NsdManager.RegistrationListener() {
                @Override public void onRegistrationFailed(NsdServiceInfo s, int e) {
                    Log.w(TAG, "_raop._tcp reg failed: " + e);
                }
                @Override public void onUnregistrationFailed(NsdServiceInfo s, int e) {}
                @Override public void onServiceRegistered(NsdServiceInfo s) {
                    Log.i(TAG, "_raop._tcp registered as '" + s.getServiceName() + "' on port " + port);
                }
                @Override public void onServiceUnregistered(NsdServiceInfo s) {}
            };
            nsdManager.registerService(raopInfo, NsdManager.PROTOCOL_DNS_SD, raopReg);
        } catch (Throwable t) {
            Log.e(TAG, "Failed to register _raop._tcp", t);
        }
    }

    private void unregisterMdns() {
        if (nsdManager == null) return;
        if (airplayReg != null) {
            try { nsdManager.unregisterService(airplayReg); } catch (Throwable ignored) {}
            airplayReg = null;
        }
        if (raopReg != null) {
            try { nsdManager.unregisterService(raopReg); } catch (Throwable ignored) {}
            raopReg = null;
        }
    }

    private static void applyTxtRecords(NsdServiceInfo info, String lines) {
        if (lines == null || lines.isEmpty()) return;
        for (String line : lines.split("\n")) {
            int eq = line.indexOf('=');
            if (eq > 0) {
                String k = line.substring(0, eq).trim();
                String v = line.substring(eq + 1).trim();
                if (!k.isEmpty()) {
                    info.setAttribute(k, v);
                }
            }
        }
    }

    @SuppressWarnings("deprecation")
    private void wakeAndLaunchUi() {
        try {
            PowerManager pm = (PowerManager) getSystemService(Context.POWER_SERVICE);
            if (pm != null) {
                wakeLock = pm.newWakeLock(
                        PowerManager.SCREEN_BRIGHT_WAKE_LOCK | PowerManager.ACQUIRE_CAUSES_WAKEUP,
                        "yansit:wakeup");
                wakeLock.setReferenceCounted(false);
                wakeLock.acquire(10_000L);
            }
            Intent i = new Intent(this, MainActivity.class);
            i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK
                    | Intent.FLAG_ACTIVITY_SINGLE_TOP
                    | Intent.FLAG_ACTIVITY_REORDER_TO_FRONT);
            startActivity(i);
        } catch (Throwable t) {
            Log.w(TAG, "wakeAndLaunchUi failed", t);
        }
    }
}
