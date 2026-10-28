//! Builds the Android app with cargo-apk2 and makes the same APK the
//! Xposed module: the hook library, the entry class (a second dex) and
//! `META-INF/xposed` go into it, then it's aligned and signed again with the
//! key cargo-apk2 used. `build [--release]`, or `run [--release]` to also
//! install and start it. Needs the Android SDK tools on PATH.

use std::env;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use indoc::indoc;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

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
    minApiVersion=101
    targetApiVersion=102
    staticScope=true
"};
const HOOK_TARGET: &str = "aarch64-linux-android";
/// API level the hook links against.
const HOOK_API: u32 = 29;
const ACTIVITY: &str = "dev.kroner.calvados/.LauncherActivity";

enum Profile {
    Dev,
    Release,
}

impl Profile {
    /// Cargo's output directory, and the profile in cargo-apk2's keystore
    /// variables.
    fn names(&self) -> (&'static str, &'static str) {
        match self {
            Self::Dev => ("debug", "DEV"),
            Self::Release => ("release", "RELEASE"),
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (install, profile) = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["build"] => (false, Profile::Dev),
        ["build", "--release"] => (false, Profile::Release),
        ["run"] => (true, Profile::Dev),
        ["run", "--release"] => (true, Profile::Release),
        _ => {
            eprintln!("usage: build|run [--release]");
            std::process::exit(2);
        }
    };
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir.ancestors().nth(2).expect("workspace root");
    let assets = manifest_dir.join("assets");
    let build_dir = workspace.join("target/xposed");
    fs::create_dir_all(&build_dir).expect("create build dir");

    build_app(workspace, &profile);
    let hook_lib = build_hook_library(workspace);
    let dex = compile_dex(&assets, &find_android_jar(), &build_dir);

    let (dir, _) = profile.names();
    let apk = workspace.join(format!("target/{dir}/apk/calvados.apk"));
    println!("Adding the Xposed module to {}", apk.display());
    let unaligned = build_dir.join("calvados.apk");
    add_module(&apk, &unaligned, &dex, &hook_lib).expect("add the module");
    run(Command::new("zipalign")
        .args(["-f", "-P", "16", "4"])
        .args([&unaligned, &apk]));
    sign_apk(&apk, &profile);
    println!("Done: {}", apk.display());
    if install {
        run(Command::new("adb").args(["install", "-r"]).arg(&apk));
        run(Command::new("adb").args(["shell", "am", "start", "-n", ACTIVITY]));
    }
}

/// Run a build tool, aborting the build on failure.
fn run(command: &mut Command) {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("failed to run {program}: {e}"));
    assert!(status.success(), "{program} failed ({status})");
}

fn build_app(workspace: &Path, profile: &Profile) {
    let mut cargo = Command::new("cargo");
    cargo
        .args(["apk2", "build", "-p", "android-app", "--manifest-path"])
        .arg(workspace.join("Cargo.toml"));
    if matches!(profile, Profile::Release) {
        cargo.arg("--release");
    }
    run(&mut cargo);
}

fn build_hook_library(workspace: &Path) -> Vec<u8> {
    println!("Building native hook library...");
    run(Command::new("cargo")
        .args(["build", "--manifest-path"])
        .arg(workspace.join("Cargo.toml"))
        .args(["-p", "android-hook", "--target", HOOK_TARGET, "--release"])
        .env("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", ndk_linker()));
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

/// The entry class, compiled against the API jar (which stays out of the
/// dex: Vector refuses modules carrying its classes).
fn compile_dex(assets: &Path, android_jar: &Path, build_dir: &Path) -> Vec<u8> {
    println!("Compiling the Xposed entry to DEX...");
    let classpath =
        env::join_paths([&assets.join("libxposed-api.jar"), android_jar]).expect("classpath");
    run(Command::new("javac")
        .args(["--release", "17", "-cp"])
        .arg(classpath)
        .arg("-d")
        .arg(build_dir)
        .arg(assets.join("java/dev/kroner/calvados/XposedEntry.java")));
    run(Command::new("d8")
        .args(["--min-api", "31", "--output"])
        .arg(build_dir)
        .arg(build_dir.join("dev/kroner/calvados/XposedEntry.class")));
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

/// cargo-apk2's v1 signature files, invalid once the APK changes.
fn is_signature(name: &str) -> bool {
    name.strip_prefix("META-INF/").is_some_and(|file| {
        !file.contains('/')
            && (file == "MANIFEST.MF"
                || [".SF", ".RSA", ".DSA", ".EC"]
                    .iter()
                    .any(|ext| file.ends_with(ext)))
    })
}

/// `apk`'s entries as they are, but its signature, plus the module's.
fn add_module(apk: &Path, out: &Path, dex: &[u8], hook_lib: &[u8]) -> zip::result::ZipResult<()> {
    let mut app = ZipArchive::new(File::open(apk)?)?;
    let mut zip = ZipWriter::new(File::create(out)?);
    for index in 0..app.len() {
        let entry = app.by_index_raw(index)?;
        if !is_signature(entry.name()) {
            zip.raw_copy_file(entry)?;
        }
    }
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let entries: [(&str, &[u8], SimpleFileOptions); 6] = [
        ("classes2.dex", dex, deflated),
        // Stored, like the app's: loaded from the APK, not extracted.
        ("lib/arm64-v8a/libcalvados_hook.so", hook_lib, stored),
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

/// The key cargo-apk2 signed with: `CARGO_APK_{DEV,RELEASE}_KEYSTORE` (and
/// `_PASSWORD`), else for dev builds its debug key.
fn sign_apk(apk: &Path, profile: &Profile) {
    let (_, name) = profile.names();
    let keystore = env::var(format!("CARGO_APK_{name}_KEYSTORE")).map(PathBuf::from);
    let password = env::var(format!("CARGO_APK_{name}_KEYSTORE_PASSWORD"));
    let (keystore, password) = match (keystore, profile) {
        (Ok(keystore), _) => (keystore, password.unwrap_or_else(|_| "android".into())),
        (Err(_), Profile::Dev) => (debug_keystore(), "android".into()),
        (Err(_), Profile::Release) => panic!("CARGO_APK_RELEASE_KEYSTORE isn't set"),
    };
    println!("Signing with {}", keystore.display());
    let pass = format!("pass:{password}");
    run(Command::new("apksigner")
        .args(["sign", "--ks"])
        .arg(&keystore)
        .args(["--ks-pass", &pass, "--key-pass", &pass])
        .arg(apk));
}

/// cargo-apk2's (ndk-build2's): `debug.keystore` in the Android user home.
fn debug_keystore() -> PathBuf {
    env::var_os("ANDROID_SDK_HOME")
        .map(|home| PathBuf::from(home).join(".android"))
        .or_else(|| env::var_os("ANDROID_USER_HOME").map(PathBuf::from))
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".android")))
        .expect("no Android user home")
        .join("debug.keystore")
}
