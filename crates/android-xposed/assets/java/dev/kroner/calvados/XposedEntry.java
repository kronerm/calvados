package dev.kroner.calvados;

import android.util.Log;

import io.github.libxposed.api.XposedModule;
import io.github.libxposed.api.XposedModuleInterface;

public class XposedEntry extends XposedModule {
    private static final String TAG = "Calvados";

    @Override
    public void onPackageLoaded(XposedModuleInterface.PackageLoadedParam param) {
        String pkg = param.getPackageName();
        log(Log.INFO, TAG, "onPackageLoaded: " + pkg + " isFirstPackage=" + param.isFirstPackage());
        if (pkg.equals("com.android.bluetooth") || pkg.equals("com.google.android.bluetooth")) {
            String libPath = getModuleApplicationInfo().nativeLibraryDir + "/libcalvados_hook.so";
            try {
                log(Log.INFO, TAG, "Loading native library from: " + libPath);
                System.load(libPath);
                log(Log.INFO, TAG, "Native library loaded successfully");
            } catch (UnsatisfiedLinkError e) {
                // Already loaded - this is fine
                log(Log.INFO, TAG, "Library already loaded or error: " + e.getMessage());
            } catch (Throwable t) {
                log(Log.ERROR, TAG, "Failed to load native library", t);
            }
        }
    }
}
