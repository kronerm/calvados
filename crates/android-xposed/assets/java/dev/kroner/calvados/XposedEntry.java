package dev.kroner.calvados;

import io.github.libxposed.api.XposedInterface;
import io.github.libxposed.api.XposedModule;
import io.github.libxposed.api.XposedModuleInterface;

public class XposedEntry extends XposedModule {
    private String nativeLibDir;

    public XposedEntry(XposedInterface base, XposedModuleInterface.ModuleLoadedParam param) {
        super(base, param);
        nativeLibDir = getApplicationInfo().nativeLibraryDir;
        log("Calvados: Module native lib dir: " + nativeLibDir);
    }

    @Override
    public void onPackageLoaded(XposedModuleInterface.PackageLoadedParam param) {
        super.onPackageLoaded(param);
        String pkg = param.getPackageName();
        log("Calvados: onPackageLoaded called for: " + pkg + " isFirstPackage=" + param.isFirstPackage());
        if (pkg.equals("com.android.bluetooth") || pkg.equals("com.google.android.bluetooth")) {
            try {
                String libPath = nativeLibDir + "/libcalvados_hook.so";
                log("Calvados: Loading native library from: " + libPath);
                System.load(libPath);
                log("Calvados: Native library loaded successfully");
            } catch (UnsatisfiedLinkError e) {
                // Already loaded - this is fine
                log("Calvados: Library already loaded or error: " + e.getMessage());
            } catch (Throwable t) {
                log("Calvados: Failed to load native library: " + t.getMessage());
            }
        }
    }
}
