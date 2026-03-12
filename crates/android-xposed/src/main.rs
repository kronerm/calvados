//! Builds the hook library and packages it as the LSPosed module APK
//! (`target/release/calvados-xposed.apk`). Needs the Android SDK tools on PATH.

use std::env;
use std::fs::{self, File};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use indoc::indoc;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const JAVA_INIT_LIST: &[u8] = indoc! {b"
    dev.kroner.calvados.XposedEntry
"};
const NATIVE_INIT_LIST: &[u8] = indoc! {b"
    libcalvados_hook.so
"};
const SCOPE_LIST: &[u8] = indoc! {b"
    com.android.bluetooth
    com.google.android.bluetooth
"};
const MODULE_PROP: &[u8] = indoc! {b"
    minApiVersion=100
    targetApiVersion=100
    staticScope=true
"};
const HOOK_TARGET: &str = "aarch64-linux-android";
/// API level the hook links against.
const HOOK_API: u32 = 29;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir.ancestors().nth(2).expect("workspace root");
    let assets = manifest_dir.join("assets");
    let android_jar = find_android_jar();
    let build_dir = env::temp_dir().join("calvados-xposed-build");
    fs::create_dir_all(&build_dir).expect("create build dir");

    let hook_lib = build_hook_library(workspace);
    let manifest = compile_manifest(&assets, &android_jar, &build_dir);
    let classes_dex = compile_dex(&assets, &android_jar, &build_dir);

    let apk = workspace.join("target/release/calvados-xposed.apk");
    println!("Creating APK: {}", apk.display());
    create_apk(&apk, &manifest, &classes_dex, &hook_lib).expect("create APK");
    align_apk(&apk);
    sign_apk(&apk, &assets.join("debug.keystore"));
    println!("Done: {}", apk.display());
}

/// Run a build tool, aborting the build on failure.
fn run(program: &str, args: &[&str]) {
    let status = Command::new(program)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {program}: {e}"));
    assert!(status.success(), "{program} failed ({status})");
}

fn path(p: &Path) -> &str {
    p.to_str().expect("UTF-8 path")
}

fn build_hook_library(workspace: &Path) -> Vec<u8> {
    println!("Building native hook library...");
    let status = Command::new("cargo")
        .args(["build", "--manifest-path"])
        .arg(workspace.join("Cargo.toml"))
        .args(["-p", "android-hook", "--target", HOOK_TARGET, "--release"])
        .env("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", ndk_linker())
        .status()
        .expect("failed to run cargo");
    assert!(status.success(), "hook build failed ({status})");
    let lib = workspace.join(format!("target/{HOOK_TARGET}/release/libcalvados_hook.so"));
    fs::read(&lib).expect("read hook library")
}

/// NDK clang for [`HOOK_TARGET`], from `ANDROID_NDK_ROOT` / `ANDROID_NDK_HOME`.
fn ndk_linker() -> PathBuf {
    let ndk = env::var("ANDROID_NDK_ROOT")
        .or_else(|_| env::var("ANDROID_NDK_HOME"))
        .unwrap_or_else(|_| "/opt/android-ndk".into());
    let host = match env::consts::OS {
        "macos" => "darwin-x86_64",
        _ => "linux-x86_64",
    };
    let linker = Path::new(&ndk)
        .join("toolchains/llvm/prebuilt")
        .join(host)
        .join(format!("bin/{HOOK_TARGET}{HOOK_API}-clang"));
    assert!(
        linker.exists(),
        "NDK linker not found: {}",
        linker.display()
    );
    linker
}

/// Binary manifest, extracted from a throwaway `aapt2 link` APK.
fn compile_manifest(assets: &Path, android_jar: &Path, build_dir: &Path) -> Vec<u8> {
    println!("Compiling AndroidManifest.xml...");
    let temp_apk = build_dir.join("manifest.apk");
    run(
        "aapt2",
        &[
            "link",
            "-I",
            path(android_jar),
            "--manifest",
            path(&assets.join("AndroidManifest.xml")),
            "-o",
            path(&temp_apk),
        ],
    );
    let mut archive = zip::ZipArchive::new(File::open(&temp_apk).expect("open manifest APK"))
        .expect("read manifest APK");
    let mut manifest = Vec::new();
    archive
        .by_name("AndroidManifest.xml")
        .expect("manifest in APK")
        .read_to_end(&mut manifest)
        .expect("read manifest");
    manifest
}

fn compile_dex(assets: &Path, android_jar: &Path, build_dir: &Path) -> Vec<u8> {
    println!("Compiling Java to DEX...");
    let classpath = format!(
        "{}:{}",
        assets.join("libxposed-api.jar").display(),
        android_jar.display()
    );
    let source = assets.join("java/dev/kroner/calvados/XposedEntry.java");
    run(
        "javac",
        &[
            "--release",
            "17",
            "-cp",
            &classpath,
            "-d",
            path(build_dir),
            path(&source),
        ],
    );
    let class = build_dir.join("dev/kroner/calvados/XposedEntry.class");
    run(
        "d8",
        &["--min-api", "24", "--output", path(build_dir), path(&class)],
    );
    fs::read(build_dir.join("classes.dex")).expect("read classes.dex")
}

/// `android.jar` of the highest installed platform.
fn find_android_jar() -> PathBuf {
    let sdk = env::var("ANDROID_SDK_ROOT")
        .or_else(|_| env::var("ANDROID_HOME"))
        .unwrap_or_else(|_| "/opt/android-sdk".into());
    fs::read_dir(Path::new(&sdk).join("platforms"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let level = entry
                .file_name()
                .to_str()?
                .strip_prefix("android-")?
                .parse::<u32>()
                .ok()?;
            let jar = entry.path().join("android.jar");
            jar.exists().then_some((level, jar))
        })
        .max_by_key(|(level, _)| *level)
        .map(|(_, jar)| jar)
        .expect("no android.jar in the SDK platforms")
}

fn create_apk(
    apk: &Path,
    manifest: &[u8],
    dex: &[u8],
    hook_lib: &[u8],
) -> zip::result::ZipResult<()> {
    fs::create_dir_all(apk.parent().expect("APK parent"))?;
    let mut zip = ZipWriter::new(File::create(apk)?);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let entries: [(&str, &[u8], SimpleFileOptions); 7] = [
        ("AndroidManifest.xml", manifest, stored),
        ("classes.dex", dex, deflated),
        ("lib/arm64-v8a/libcalvados_hook.so", hook_lib, deflated),
        ("META-INF/xposed/java_init.list", JAVA_INIT_LIST, stored),
        ("META-INF/xposed/native_init.list", NATIVE_INIT_LIST, stored),
        ("META-INF/xposed/scope.list", SCOPE_LIST, stored),
        ("META-INF/xposed/module.prop", MODULE_PROP, stored),
    ];
    for (name, data, options) in entries {
        zip.start_file(name, options)?;
        zip.write_all(data)?;
    }
    zip.finish()?;
    Ok(())
}

fn align_apk(apk: &Path) {
    println!("Aligning APK...");
    let aligned = apk.with_extension("apk.aligned");
    // `-P 16`: 16 KB page alignment for uncompressed .so (build-tools ≥ 35).
    run(
        "zipalign",
        &["-f", "-P", "16", "4", path(apk), path(&aligned)],
    );
    fs::rename(&aligned, apk).expect("replace APK with aligned one");
}

fn sign_apk(apk: &Path, keystore: &Path) {
    if !keystore.exists() {
        println!("Generating debug keystore...");
        run(
            "keytool",
            &[
                "-genkey",
                "-v",
                "-keystore",
                path(keystore),
                "-alias",
                "debug",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "10000",
                "-storepass",
                "android",
                "-keypass",
                "android",
                "-dname",
                "CN=Debug,O=Calvados,C=US",
            ],
        );
    }
    println!("Signing APK...");
    run(
        "apksigner",
        &[
            "sign",
            "--ks",
            path(keystore),
            "--ks-pass",
            "pass:android",
            "--key-pass",
            "pass:android",
            path(apk),
        ],
    );
}
