package dev.kroner.calvados;

import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Bundle;
import android.os.PowerManager;
import android.provider.Settings;

import java.util.Arrays;

/** Invisible launcher: requests permissions, starts the service, finishes. */
public class LauncherActivity extends Activity {
    private static final String[] PERMISSIONS = {
        "android.permission.BLUETOOTH_CONNECT",
        "android.permission.BLUETOOTH_SCAN",
        "android.permission.POST_NOTIFICATIONS",
        // Calls take the audio when answered.
        "android.permission.READ_PHONE_STATE",
    };

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        boolean granted = Arrays.stream(PERMISSIONS)
            .allMatch(p -> checkSelfPermission(p) == PackageManager.PERMISSION_GRANTED);
        if (granted) {
            startServiceAndFinish();
        } else {
            requestPermissions(PERMISSIONS, 1);
        }
    }

    @Override
    public void onRequestPermissionsResult(int requestCode, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(requestCode, permissions, results);
        startServiceAndFinish(); // with whatever was granted
    }

    private void startServiceAndFinish() {
        if (!getSystemService(PowerManager.class).isIgnoringBatteryOptimizations(getPackageName())) {
            startActivity(new Intent(
                Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
                Uri.parse("package:" + getPackageName())));
        }
        startForegroundService(new Intent(this, PersistentService.class));
        finish();
    }
}
