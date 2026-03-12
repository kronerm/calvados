use std::ffi::{CStr, c_int, c_void};

use libc::{dl_phdr_info, size_t};

/// A loaded shared object (via `dl_iterate_phdr`).
pub struct ModuleInfo {
    pub path: String,
    pub base_address: usize,
}

pub fn get_module_info(name: &str) -> Option<ModuleInfo> {
    struct CallbackData {
        target_name: String,
        result: Option<ModuleInfo>,
    }

    const STOP: c_int = 1;
    const CONTINUE: c_int = 0;

    unsafe extern "C" fn callback(
        info: *mut dl_phdr_info,
        _size: size_t,
        data: *mut c_void,
    ) -> c_int {
        let cb_data = unsafe { &mut *(data as *mut CallbackData) };
        let info = unsafe { &*info };

        if info.dlpi_name.is_null() {
            return CONTINUE;
        }

        let path = unsafe { CStr::from_ptr(info.dlpi_name) }.to_string_lossy();

        if path.contains(&cb_data.target_name) {
            cb_data.result = Some(ModuleInfo {
                base_address: info.dlpi_addr as usize,
                path: path.into_owned(),
            });
            return STOP;
        }

        CONTINUE
    }

    let mut data = CallbackData {
        target_name: name.to_string(),
        result: None,
    };

    unsafe { libc::dl_iterate_phdr(Some(callback), &mut data as *mut _ as *mut c_void) };

    data.result
}
