//! "Start after sign-in" via the per-user Run key; no admin rights needed.

use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

use crate::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "GameSuspend";

pub fn is_enabled() -> bool {
    let key = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    status == 0
}

/// Enabling always rewrites the path, which also fixes it after the exe was moved.
pub fn set(enabled: bool) -> Result<(), String> {
    let key = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    let status = if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let command = wide(&format!("\"{}\"", exe.display()));
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * 2) as u32,
            )
        }
    } else {
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) } {
            ERROR_FILE_NOT_FOUND => 0,
            status => status,
        }
    };
    if status == 0 {
        Ok(())
    } else {
        Err(format!(
            "can't change starting with Windows (error {status})"
        ))
    }
}
