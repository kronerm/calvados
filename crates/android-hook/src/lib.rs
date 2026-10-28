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
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer as _};

use crate::module_info::get_module_info;

mod module_info;

mod bta_dm_set_local_di_record;
mod l2c_fcr_chk_chan_modes;
mod remote_suspend;

/// Find a symbol's address offset in an ELF file by name.
fn find_symbol_offset(elf: &object::File<'_>, symbol_name: &str) -> Option<u64> {
    elf.symbols()
        .find(|symbol| {
            matches!(symbol.kind(), SymbolKind::Text)
                && symbol.name().is_ok_and(|name| name.contains(symbol_name))
        })
        .map(|symbol| symbol.address())
}

/// The same for an exact (mangled) name.
fn find_exact_symbol_offset(elf: &object::File<'_>, symbol_name: &str) -> Option<u64> {
    elf.symbols()
        .find(|symbol| {
            matches!(symbol.kind(), SymbolKind::Text) && symbol.name() == Ok(symbol_name)
        })
        .map(|symbol| symbol.address())
}

/// Install one hook, storing the original function pointer.
///
/// # Safety
/// `replacement` must have the same signature as the `F` stored in `original`.
unsafe fn install_hook<F: Copy>(
    hook_fn: HookFn,
    base: usize,
    offset: Option<u64>,
    replacement: *mut c_void,
    original_store: &OnceLock<F>,
    name: &str,
) {
    let Some(offset) = offset else {
        tracing::error!(symbol = name, "symbol not found");
        return;
    };
    let target = (base + offset as usize) as *mut c_void;
    let mut original: *mut c_void = ptr::null_mut();

    let result = unsafe { hook_fn(target, replacement, &mut original) };

    if result == 0 {
        // A second load of the library keeps the first original.
        let _ = original_store.set(unsafe { std::mem::transmute_copy(&original) });
        tracing::info!(function_name=name, offset=%format_args!("{offset:#x}"), "hooked!");
    } else {
        tracing::error!(
            function_name = name,
            hook_fn_return = result,
            "failed to hook :("
        );
    }
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
    let elf_file = object::File::parse(elf_data.as_slice())?;

    // Some Android builds strip symbols into a compressed .gnu_debugdata section.
    let gnu_debugdata = elf_file
        .section_by_name(".gnu_debugdata")
        .and_then(|section| {
            let mut reader = XzReader::new(section.data().ok()?, false);
            let mut decompressed = vec![];
            reader.read_to_end(&mut decompressed).ok()?;
            Some(decompressed)
        });
    let gnu_debugdata_elf = gnu_debugdata
        .as_ref()
        .and_then(|data| object::File::parse(data.as_slice()).ok());

    // Look up symbol in main ELF, falling back to .gnu_debugdata.
    let find_symbol = |name: &str| -> Option<u64> {
        find_symbol_offset(&elf_file, name).or_else(|| {
            gnu_debugdata_elf
                .as_ref()
                .and_then(|f| find_symbol_offset(f, name))
        })
    };
    let find_exact = |name: &str| -> Option<u64> {
        find_exact_symbol_offset(&elf_file, name).or_else(|| {
            gnu_debugdata_elf
                .as_ref()
                .and_then(|f| find_exact_symbol_offset(f, name))
        })
    };

    // Functions the hooks call, not hook.
    let function = |name: &str| {
        let address = find_exact(name).map(|offset| module.base_address + offset as usize);
        if address.is_none() {
            tracing::error!(symbol = name, "symbol not found");
        }
        address
    };
    if let Some(clear) = function(remote_suspend::CLEAR_REMOTE_SUSPEND) {
        let _ = remote_suspend::CLEAR.set(unsafe {
            std::mem::transmute::<usize, remote_suspend::ClearRemoteSuspend>(clear)
        });
    }
    if let Some(start) = function(remote_suspend::START) {
        let _ = remote_suspend::START_FN
            .set(unsafe { std::mem::transmute::<usize, remote_suspend::Start>(start) });
    }

    unsafe {
        install_hook(
            hook_fn,
            module.base_address,
            find_symbol(l2c_fcr_chk_chan_modes::SYMBOL),
            l2c_fcr_chk_chan_modes::l2c_fcr_chk_chan_modes as *mut c_void,
            &l2c_fcr_chk_chan_modes::ORIGINAL_FN,
            l2c_fcr_chk_chan_modes::SYMBOL,
        );
        install_hook(
            hook_fn,
            module.base_address,
            find_symbol(bta_dm_set_local_di_record::SYMBOL),
            bta_dm_set_local_di_record::BTA_DmSetLocalDiRecord as *mut c_void,
            &bta_dm_set_local_di_record::ORIGINAL_FN,
            bta_dm_set_local_di_record::SYMBOL,
        );
        // The interface's member where it exists (Android 17): the free
        // function forwards to it.
        match find_exact(remote_suspend::IMPL_DATA_WRITE) {
            Some(offset) => install_hook(
                hook_fn,
                module.base_address,
                Some(offset),
                remote_suspend::calvados_impl_l2ca_data_write as *mut c_void,
                &remote_suspend::ORIGINAL_IMPL_DATA_WRITE,
                remote_suspend::IMPL_DATA_WRITE,
            ),
            None => install_hook(
                hook_fn,
                module.base_address,
                find_exact(remote_suspend::DATA_WRITE),
                remote_suspend::L2CA_DataWrite as *mut c_void,
                &remote_suspend::ORIGINAL_DATA_WRITE,
                remote_suspend::DATA_WRITE,
            ),
        }
    }

    Ok(())
}

/// # Safety
/// `name` must be either a valid C string or null.
#[unsafe(no_mangle)]
#[tracing::instrument]
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
#[tracing::instrument]
pub unsafe extern "C" fn native_init(entries: *const LSPosedNativeApiEntries) -> OnModuleLoaded {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=trace,info", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(
            paranoid_android::layer(env!("CARGO_CRATE_NAME"))
                .with_span_events(FmtSpan::CLOSE)
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
