//! Read-only registry access.
//!
//! Shared by the display adapter probe and the text service probe. Both walk a
//! class key and read values out of its subkeys, and neither can afford to load
//! anything to do it: the adapter probe runs before a driver is chosen, and the
//! text service probe runs before the process is hardened.

use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
use windows_sys::Win32::Foundation::ERROR_NO_MORE_ITEMS;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows_sys::Win32::System::Registry::HKEY;
use windows_sys::Win32::System::Registry::KEY_READ;
use windows_sys::Win32::System::Registry::RegCloseKey;
use windows_sys::Win32::System::Registry::RegEnumKeyExW;
use windows_sys::Win32::System::Registry::RegOpenKeyExW;
use windows_sys::Win32::System::Registry::RegQueryValueExW;

use crate::util::win32::Win32Status;

pub use windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE;

const REG_EXPAND_SZ: u32 = 2;

pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Decode a `REG_SZ` or `REG_MULTI_SZ` payload into its component strings.
///
/// A value that is single on one machine can be multi on another, so the same
/// splitter serves both: each form is null terminated and the multi form only
/// adds a second terminator.
pub fn decode_strings(units: &[u16]) -> Vec<String> {
    units.split(|unit| *unit == 0).filter(|part| !part.is_empty()).map(String::from_utf16_lossy).collect()
}

/// Substitute `%VAR%` references, which `REG_EXPAND_SZ` values carry unexpanded.
///
/// Returns the input unchanged when the expansion fails, which only leaves a
/// path that will not resolve, exactly as the unexpanded form already would.
fn expand(value: &str) -> String {
    let source = wide(value);
    // SAFETY: a null destination with a zero length is the documented way to
    // ask for the required buffer size, in characters including the terminator.
    let needed = unsafe { ExpandEnvironmentStringsW(source.as_ptr(), std::ptr::null_mut(), 0) };
    if needed == 0 {
        return value.to_string();
    }

    let mut buffer = vec![0u16; needed as usize];
    // SAFETY: `buffer` holds `needed` characters, the size the call above asked
    // for, and `source` is a NUL-terminated UTF-16 string that outlives it.
    let written = unsafe { ExpandEnvironmentStringsW(source.as_ptr(), buffer.as_mut_ptr(), needed) };
    if written == 0 || written > needed {
        return value.to_string();
    }

    // The count includes the terminator, which is not part of the string.
    String::from_utf16_lossy(&buffer[..written.saturating_sub(1) as usize])
}

/// Owns an open registry key so every early return closes it.
pub struct RegKey(HKEY);

impl RegKey {
    pub fn open(parent: HKEY, path: &str) -> Result<Self, Win32Status> {
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: `path` is NUL-terminated UTF-16 that outlives the call, and
        // `key` is only read when the call reports success.
        let status = unsafe { RegOpenKeyExW(parent, wide(path).as_ptr(), 0, KEY_READ, &mut key) };
        if status == ERROR_SUCCESS { Ok(Self(key)) } else { Err(Win32Status::new(status)) }
    }

    pub fn raw(&self) -> HKEY {
        self.0
    }

    /// Raw value bytes and their registry type, or `None` when the value is
    /// absent. The empty value name reads a key's default value.
    pub fn query(&self, name: &str) -> Option<(u32, Vec<u8>)> {
        let name = wide(name);
        let mut reg_type: u32 = 0;
        let mut size: u32 = 0;
        // SAFETY: a null data pointer asks for the required size, which is what
        // `size` receives; `name` outlives both calls.
        let status = unsafe {
            RegQueryValueExW(self.0, name.as_ptr(), std::ptr::null(), &mut reg_type, std::ptr::null_mut(), &mut size)
        };
        if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
            return None;
        }

        let mut data = vec![0u8; size as usize];
        // SAFETY: `data` holds `size` bytes, the size the call above reported.
        let status = unsafe {
            RegQueryValueExW(self.0, name.as_ptr(), std::ptr::null(), &mut reg_type, data.as_mut_ptr(), &mut size)
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        data.truncate(size as usize);
        Some((reg_type, data))
    }

    /// A string value, decoded from either the single or the multi form, with
    /// environment references expanded when the value type calls for it.
    pub fn query_strings(&self, name: &str) -> Vec<String> {
        let Some((reg_type, data)) = self.query(name) else {
            return Vec::new();
        };
        let units: Vec<u16> = data.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        let strings = decode_strings(&units);
        if reg_type == REG_EXPAND_SZ { strings.iter().map(|value| expand(value)).collect() } else { strings }
    }

    pub fn subkey_names(&self) -> Result<Vec<String>, Win32Status> {
        let mut names = Vec::new();
        // Registry key names are capped at 255 characters plus a terminator.
        let mut buffer = [0u16; 256];
        for index in 0.. {
            let mut length = buffer.len() as u32;
            // SAFETY: `buffer` has `length` characters of room, and every
            // optional out parameter is null.
            let status = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    buffer.as_mut_ptr(),
                    &mut length,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            match status {
                ERROR_SUCCESS => names.push(String::from_utf16_lossy(&buffer[..length as usize])),
                ERROR_NO_MORE_ITEMS => break,
                other => return Err(Win32Status::new(other)),
            }
        }
        Ok(names)
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from a successful RegOpenKeyExW and is closed
        // exactly once, since RegKey is not Copy or Clone.
        unsafe { RegCloseKey(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }

    #[test]
    fn single_string_value_decodes_to_one_entry() {
        let mut units = utf16(r"C:\drivers\nv-vk64.json");
        units.push(0);

        assert_eq!(decode_strings(&units), vec![r"C:\drivers\nv-vk64.json".to_string()]);
    }

    #[test]
    fn multi_string_value_decodes_to_every_entry() {
        let mut units = Vec::new();
        for path in [r"C:\a.json", r"C:\b.json"] {
            units.extend(utf16(path));
            units.push(0);
        }
        units.push(0);

        assert_eq!(decode_strings(&units), vec![r"C:\a.json".to_string(), r"C:\b.json".to_string()]);
    }

    #[test]
    fn empty_value_decodes_to_no_entries() {
        assert!(decode_strings(&[]).is_empty());
        assert!(decode_strings(&[0, 0]).is_empty());
    }

    #[test]
    fn environment_references_expand() {
        let expanded = expand(r"%SystemRoot%\system32\imm32.dll");

        assert!(!expanded.contains('%'), "expansion left a reference behind: {expanded}");
        assert!(expanded.to_lowercase().ends_with(r"\system32\imm32.dll"));
    }

    #[test]
    fn a_path_with_no_references_survives_expansion_unchanged() {
        assert_eq!(expand(r"C:\Program Files\Windows NT\x.dll"), r"C:\Program Files\Windows NT\x.dll");
    }
}
