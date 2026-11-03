package com.yansit.tv;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.util.Log;

public class BootReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context context, Intent intent) {
        if (intent == null || intent.getAction() == null) return;
        Log.i("yansit", "BootReceiver triggered: " + intent.getAction());
        try {
            Intent svc = new Intent(context, AirPlayService.class);
            context.startForegroundService(svc);
        } catch (Throwable t) {
            Log.w("yansit", "Failed to start AirPlayService on boot", t);
        }
    }
}
