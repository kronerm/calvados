//! LSPosed native hook for the system Bluetooth stack (Android only).
#![cfg(target_os = "android")]

use std::ffi::{CStr, c_char, c_int, c_void};
use std::io::Read;
use std::ptr;
use std::sync::OnceLock;

use anyhow::Context as _;
use lzma_rust2::XzReader;
use object::{Object as _, ObjectSection as _, ObjectSymbol as _, SymbolKind};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer as _};

use crate::module_info::get_module_info;

mod module_info;

mod bta_dm_set_local_di_record;
mod l2c_fcr_chk_chan_modes;
mod remote_suspend;

/// How a symbol's name is matched.
#[derive(Clone, Copy)]
enum Name<'a> {
    /// A C function, perhaps mangled.
    Containing(&'a str),
    /// A mangled C++ name.
    Exact(&'a str),
}

impl Name<'_> {
    fn as_str(&self) -> &str {
        match self {
            Self::Containing(name) | Self::Exact(name) => name,
        }
    }

    fn matches(&self, symbol: &str) -> bool {
        match self {
            Self::Containing(name) => symbol.contains(name),
            Self::Exact(name) => symbol == *name,
        }
    }
}

/// A loaded Bluetooth library and its symbols.
struct Library<'a> {
    hook_fn: HookFn,
    base: usize,
    /// The file, then its `.gnu_debugdata` (some builds strip the symbols
    /// into it).
    elfs: Vec<object::File<'a>>,
}

impl Library<'_> {
    /// A function's address, from the first ELF that has it.
    fn find(&self, name: Name<'_>) -> Option<usize> {
        self.elfs.iter().find_map(|elf| {
            elf.symbols()
                .find(|symbol| {
                    matches!(symbol.kind(), SymbolKind::Text)
                        && symbol.name().is_ok_and(|symbol| name.matches(symbol))
                })
                .map(|symbol| self.base + symbol.address() as usize)
        })
    }

    fn find_or_log(&self, name: Name<'_>) -> Option<usize> {
        let address = self.find(name);
        if address.is_none() {
            tracing::error!(symbol = name.as_str(), "symbol not found");
        }
        address
    }

    /// Store a function the hooks call.
    ///
    /// # Safety
    /// `F` must be the function pointer type of `name`.
    unsafe fn resolve<F: Copy>(&self, name: &str, store: &OnceLock<F>) {
        const { assert!(size_of::<F>() == size_of::<usize>()) };
        if let Some(address) = self.find_or_log(Name::Exact(name)) {
            let _ = store.set(unsafe { std::mem::transmute_copy(&address) });
        }
    }

    /// Replace a function, storing the original.
    ///
    /// # Safety
    /// `F` must be the function pointer type of `name`.
    unsafe fn hook<F: Copy>(&self, name: Name<'_>, replacement: F, original: &OnceLock<F>) {
        const { assert!(size_of::<F>() == size_of::<*mut c_void>()) };
        let Some(target) = self.find_or_log(name) else {
            return;
        };
        let replacement: *mut c_void = unsafe { std::mem::transmute_copy(&replacement) };
        let mut backup: *mut c_void = ptr::null_mut();
        let result = unsafe { (self.hook_fn)(target as *mut c_void, replacement, &mut backup) };
        if result == 0 {
            // A second load of the library keeps the first original.
            let _ = original.set(unsafe { std::mem::transmute_copy(&backup) });
            tracing::info!(
                symbol = name.as_str(),
                offset = %format_args!("{:#x}", target - self.base),
                "hooked"
            );
        } else {
            tracing::error!(symbol = name.as_str(), result, "hooking failed");
        }
    }
}

/// `.gnu_debugdata`, decompressed (xz).
fn gnu_debugdata(elf: &object::File<'_>) -> Option<Vec<u8>> {
    let data = elf.section_by_name(".gnu_debugdata")?.data().ok()?;
    let mut decompressed = Vec::new();
    XzReader::new(data, false)
        .read_to_end(&mut decompressed)
        .ok()?;
    Some(decompressed)
}

/// Install hooks into a Bluetooth library.
#[tracing::instrument]
fn hook_library(library_name: &str) -> anyhow::Result<()> {
    let Some(&hook_fn) = HOOK_FUNC.get() else {
        anyhow::bail!("hook_func not initialized");
    };

    let module = get_module_info(library_name).context("failed to get module info")?;

    tracing::info!(
        library_name,
        module.path,
        module.base_address=%format_args!("{:#x}", module.base_address),
        "module found",
    );

    let elf_data = std::fs::read(&module.path)?;
    let elf = object::File::parse(elf_data.as_slice())?;
    let debugdata = gnu_debugdata(&elf);
    let library = Library {
        hook_fn,
        base: module.base_address,
        elfs: std::iter::once(elf)
            .chain(
                debugdata
                    .as_deref()
                    .and_then(|data| object::File::parse(data).ok()),
            )
            .collect(),
    };

    unsafe {
        library.resolve(remote_suspend::CLEAR_REMOTE_SUSPEND, &remote_suspend::CLEAR);
        library.resolve(remote_suspend::START, &remote_suspend::START_FN);
        library.hook(
            Name::Containing(l2c_fcr_chk_chan_modes::SYMBOL),
            l2c_fcr_chk_chan_modes::l2c_fcr_chk_chan_modes as l2c_fcr_chk_chan_modes::Signature,
            &l2c_fcr_chk_chan_modes::ORIGINAL_FN,
        );
        library.hook(
            Name::Containing(bta_dm_set_local_di_record::SYMBOL),
            bta_dm_set_local_di_record::bta_dm_set_local_di_record
                as bta_dm_set_local_di_record::Signature,
            &bta_dm_set_local_di_record::ORIGINAL_FN,
        );
        // The interface's member where it exists (Android 17): the free
        // function forwards to it.
        let member = Name::Exact(remote_suspend::IMPL_DATA_WRITE);
        match library.find(member) {
            Some(_) => library.hook(
                member,
                remote_suspend::impl_l2ca_data_write as remote_suspend::ImplDataWrite,
                &remote_suspend::ORIGINAL_IMPL_DATA_WRITE,
            ),
            None => library.hook(
                Name::Exact(remote_suspend::DATA_WRITE),
                remote_suspend::l2ca_data_write as remote_suspend::DataWrite,
                &remote_suspend::ORIGINAL_DATA_WRITE,
            ),
        }
    }

    Ok(())
}

/// # Safety
/// `name` must be either a valid C string or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_library_loaded(name: *const c_char, _handle: *mut c_void) {
    if name.is_null() {
        return;
    }

    let name_str = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(error) => {
            tracing::error!(?error, "name not valid utf8");
            return;
        }
    };

    const TARGET_LIBRARIES: &[&str] = &["libbluetooth_jni.so", "libbluetooth_qti.so"];

    TARGET_LIBRARIES
        .iter()
        .filter(|library| name_str.contains(*library))
        .for_each(|library| {
            tracing::info!(library, "library loaded");
            if let Err(error) = hook_library(library) {
                tracing::error!(?error, "couldn't hook library");
            }
        });
}

#[repr(C)]
pub struct LSPosedNativeApiEntries {
    pub version: u32,
    pub hook_func: *mut c_void,
    pub unhook_func: *mut c_void,
}

type HookFn = unsafe extern "C" fn(
    function_to_replace_ptr: *mut c_void,
    replacement_function_ptr: *mut c_void,
    backup_ptr: *mut *mut c_void,
) -> c_int;

type OnModuleLoaded = unsafe extern "C" fn(name: *const c_char, handle: *mut c_void);

/// Stored hook function from LSPosed
static HOOK_FUNC: OnceLock<HookFn> = OnceLock::new();

/// LSPosed entry point.
///
/// # Safety
/// `entries` must be a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn native_init(entries: *const LSPosedNativeApiEntries) -> OnModuleLoaded {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=trace,info", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(
            paranoid_android::layer(env!("CARGO_CRATE_NAME"))
                .with_file(true)
                .with_line_number(true)
                .with_thread_names(true)
                .with_filter(LevelFilter::TRACE),
        )
        .init();

    let entries = unsafe { &*entries };
    tracing::info!(
        lsposed.api.version = entries.version,
        "native hook initialized"
    );

    let _ = HOOK_FUNC.set(unsafe { std::mem::transmute::<*mut c_void, HookFn>(entries.hook_func) });

    on_library_loaded
}
