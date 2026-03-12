package dev.kroner.calvados;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/** Starts the service after boot and after an app update (manifest filter). */
public class StartReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context context, Intent intent) {
        context.startForegroundService(new Intent(context, PersistentService.class));
    }
}
