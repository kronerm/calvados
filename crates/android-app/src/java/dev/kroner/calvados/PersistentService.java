package dev.kroner.calvados;

import android.app.Service;
import android.bluetooth.BluetoothAdapter;
import android.bluetooth.BluetoothDevice;
import android.bluetooth.BluetoothProfile;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.media.AudioManager;
import android.media.AudioPlaybackConfiguration;
import android.os.IBinder;
import android.telephony.TelephonyManager;

import java.util.List;

/** Lifecycle stub; everything else lives in Rust (service.rs). */
public class PersistentService extends Service implements BluetoothProfile.ServiceListener {
    static {
        System.loadLibrary("calvados");
    }

    private native void nativeOnCreate();
    private static native void nativeOnIntent(Intent intent);
    private static native void nativeOnProfileProxy(int profile, BluetoothProfile proxy);
    private static native void nativeOnPlayback(List<AudioPlaybackConfiguration> configs);
    private static native void nativeOnDestroy();

    private final BroadcastReceiver aclReceiver = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            nativeOnIntent(intent);
        }
    };

    // An abstract class, so it can't be created over JNI.
    private final AudioManager.AudioPlaybackCallback playbackCallback =
        new AudioManager.AudioPlaybackCallback() {
            @Override
            public void onPlaybackConfigChanged(List<AudioPlaybackConfiguration> configs) {
                nativeOnPlayback(configs);
            }
        };

    @Override
    public void onCreate() {
        super.onCreate();
        nativeOnCreate();
        IntentFilter filter = new IntentFilter(BluetoothDevice.ACTION_ACL_CONNECTED);
        filter.addAction(BluetoothDevice.ACTION_ACL_DISCONNECTED);
        filter.addAction(BluetoothAdapter.ACTION_STATE_CHANGED);
        filter.addAction(TelephonyManager.ACTION_PHONE_STATE_CHANGED);
        registerReceiver(aclReceiver, filter);
        getSystemService(AudioManager.class).registerAudioPlaybackCallback(playbackCallback, null);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        nativeOnIntent(intent);
        return START_STICKY;
    }

    @Override
    public void onServiceConnected(int profile, BluetoothProfile proxy) {
        nativeOnProfileProxy(profile, proxy);
    }

    @Override
    public void onServiceDisconnected(int profile) {}

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    @Override
    public void onDestroy() {
        getSystemService(AudioManager.class).unregisterAudioPlaybackCallback(playbackCallback);
        unregisterReceiver(aclReceiver);
        nativeOnDestroy();
        super.onDestroy();
    }
}
