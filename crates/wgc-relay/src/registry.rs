use std::ptr::null;
use std::ptr::null_mut;

use wgc_relay_protocol::GAME_CENTER_DIR;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::HKEY;
use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;
use windows_sys::Win32::System::Registry::KEY_SET_VALUE;
use windows_sys::Win32::System::Registry::REG_OPTION_NON_VOLATILE;
use windows_sys::Win32::System::Registry::REG_SZ;
use windows_sys::Win32::System::Registry::RegCloseKey;
use windows_sys::Win32::System::Registry::RegCreateKeyExW;
use windows_sys::Win32::System::Registry::RegSetValueExW;

use crate::error::RelayError;

/// Writes the Uninstall entry `wgc_api.dll` reads to find WGC. It checks HKLM
/// then HKCU; HKCU needs no elevation. Setting the same value again is a
/// no-op, so every launch writes it.
pub fn register_fake_install() -> Result<(), RelayError> {
    let subkey = wide(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Wargaming.net Game Center");
    let value_name = wide("DisplayIcon");
    let icon = wide(&format!(r"{GAME_CENTER_DIR}\wgc.exe"));

    let mut key: HKEY = null_mut();
    // SAFETY: every pointer refers to a live NUL-terminated buffer or out slot.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null(),
            &mut key,
            null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(RelayError::Win32 { call: "RegCreateKeyExW", code: status });
    }

    // SAFETY: `key` is open; the data length counts the terminating NUL.
    let status =
        unsafe { RegSetValueExW(key, value_name.as_ptr(), 0, REG_SZ, icon.as_ptr().cast(), (icon.len() * 2) as u32) };
    // SAFETY: closes the key opened above exactly once.
    unsafe { RegCloseKey(key) };
    if status != ERROR_SUCCESS {
        return Err(RelayError::Win32 { call: "RegSetValueExW", code: status });
    }

    Ok(())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
