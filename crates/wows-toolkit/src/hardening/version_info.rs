//! Read a binary's version resource.
//!
//! Enough to say who shipped a module, which is what both consumers need: the
//! text service probe to decide whether Code Integrity Guard would break
//! someone's typing, and the crash report to name a foreign module's origin.
//!
//! This is not a signature check. The kernel already makes the trust decision,
//! and `WinVerifyTrust` would add substantial code for no diagnostic gain.

use std::path::Path;

/// The `CompanyName` string from a binary's version resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompanyName(String);

impl CompanyName {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this names Microsoft.
    ///
    /// Windows ships components under both `Microsoft Corporation` and a bare
    /// `Microsoft`, so equality alone is not enough. A bare prefix is too much:
    /// it would accept `MicrosoftFoo Ltd`, so the name must either be exactly
    /// `Microsoft` or continue with a word boundary.
    pub fn is_microsoft(&self) -> bool {
        let name = self.0.trim().to_lowercase();
        name == "microsoft" || name.starts_with("microsoft ")
    }
}

impl std::fmt::Display for CompanyName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The `ProductName` string from a binary's version resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProductName(String);

impl ProductName {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProductName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a binary claims about its own origin.
///
/// Each field is independently optional: a binary can carry a version resource
/// that names one and not the other, and an absent name is not the same as an
/// empty one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionInfo {
    pub company: Option<CompanyName>,
    pub product: Option<ProductName>,
}

impl VersionInfo {
    pub fn is_empty(&self) -> bool {
        self.company.is_none() && self.product.is_none()
    }
}

/// Read `path`'s version resource, or `None` when it has none.
#[cfg(windows)]
pub fn read(path: &Path) -> Option<VersionInfo> {
    windows_version_info::read(path)
}

#[cfg(not(windows))]
pub fn read(_path: &Path) -> Option<VersionInfo> {
    None
}

#[cfg(windows)]
mod windows_version_info {
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::GetFileVersionInfoSizeW;
    use windows_sys::Win32::Storage::FileSystem::GetFileVersionInfoW;
    use windows_sys::Win32::Storage::FileSystem::VerQueryValueW;

    use crate::util::registry::wide;

    use super::CompanyName;
    use super::ProductName;
    use super::VersionInfo;

    /// A language and code page pair from `\VarFileInfo\Translation`, which
    /// names the sub-block the string values actually live under.
    #[derive(Clone, Copy)]
    struct Translation {
        language: u16,
        code_page: u16,
    }

    impl Translation {
        fn sub_block(self, field: &str) -> String {
            format!(r"\StringFileInfo\{:04x}{:04x}\{field}", self.language, self.code_page)
        }
    }

    /// The version resource bytes, kept alive for as long as pointers into it
    /// are read.
    struct VersionBlock(Vec<u8>);

    impl VersionBlock {
        fn read(path: &Path) -> Option<Self> {
            let path = wide(&path.to_string_lossy());
            let mut handle: u32 = 0;
            // SAFETY: `path` is a NUL-terminated UTF-16 string that outlives the
            // call. The handle out parameter is documented as ignored.
            let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut handle) };
            if size == 0 {
                return None;
            }

            let mut data = vec![0u8; size as usize];
            // SAFETY: `data` holds `size` bytes, the size the call above asked
            // for.
            let ok = unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) };
            (ok != 0).then_some(Self(data))
        }

        /// The first translation the resource declares.
        ///
        /// A resource can declare several, but the strings are the same claim in
        /// different languages, and the first is the one the shipping locale
        /// wrote.
        fn translation(&self) -> Option<Translation> {
            let name = wide(r"\VarFileInfo\Translation");
            let mut value: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut length: u32 = 0;
            // SAFETY: the block came from GetFileVersionInfoW, and `value` is
            // only dereferenced below after a success return with a length that
            // covers the two u16 fields being read.
            let ok = unsafe { VerQueryValueW(self.0.as_ptr().cast(), name.as_ptr(), &mut value, &mut length) };
            if ok == 0 || value.is_null() || (length as usize) < std::mem::size_of::<u16>() * 2 {
                return None;
            }

            // SAFETY: the pointer is into `self.0`, which outlives this read,
            // and the length check above proves both fields are present. The
            // pair is read unaligned because the resource makes no alignment
            // promise about where a sub-block starts.
            let (language, code_page) = unsafe {
                let units = value.cast::<u16>();
                (units.read_unaligned(), units.add(1).read_unaligned())
            };
            Some(Translation { language, code_page })
        }

        /// How many `u16`s remain in this block from `value` onward, or `None`
        /// when `value` does not point inside the block at all.
        fn units_from(&self, value: *const core::ffi::c_void) -> Option<usize> {
            let start = self.0.as_ptr() as usize;
            let end = start.checked_add(self.0.len())?;
            let offset = (value as usize).checked_sub(start)?;
            if value as usize >= end {
                return None;
            }
            Some((self.0.len() - offset) / std::mem::size_of::<u16>())
        }

        fn string(&self, sub_block: &str) -> Option<String> {
            let name = wide(sub_block);
            let mut value: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut length: u32 = 0;
            // SAFETY: as above; `length` receives the character count of the
            // string this sub-block holds.
            let ok = unsafe { VerQueryValueW(self.0.as_ptr().cast(), name.as_ptr(), &mut value, &mut length) };
            if ok == 0 || value.is_null() || length == 0 {
                return None;
            }

            // The resource belongs to a third-party module, so its declared
            // length is untrusted input. `version.dll` validates sub-block
            // extents, but clamping to what the block actually holds costs
            // nothing and makes the read sound without relying on that.
            let length = (length as usize).min(self.units_from(value)?);

            // SAFETY: the pointer is into `self.0`, and `length` is clamped
            // above to the characters that remain in the block from it.
            let units = unsafe { std::slice::from_raw_parts(value.cast::<u16>(), length) };
            // The reported length includes the terminator, and some resources
            // pad with further nulls, so cut at the first one.
            let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
            let text = String::from_utf16_lossy(&units[..end]);
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
    }

    pub(super) fn read(path: &Path) -> Option<VersionInfo> {
        let block = VersionBlock::read(path)?;
        let translation = block.translation()?;

        let info = VersionInfo {
            company: block.string(&translation.sub_block("CompanyName")).map(CompanyName::new),
            product: block.string(&translation.sub_block("ProductName")).map(ProductName::new),
        };
        // A resource that named neither is indistinguishable from no resource
        // at all, and reporting it as present would make an unidentifiable
        // module look identified.
        (!info.is_empty()).then_some(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microsoft_is_recognised_in_both_forms_windows_ships() {
        assert!(CompanyName::new("Microsoft Corporation").is_microsoft());
        assert!(CompanyName::new("Microsoft").is_microsoft());
        assert!(CompanyName::new("microsoft corporation").is_microsoft());
    }

    #[test]
    fn other_vendors_are_not_microsoft() {
        assert!(!CompanyName::new("NVIDIA Corporation").is_microsoft());
        assert!(!CompanyName::new("Advanced Micro Devices, Inc.").is_microsoft());
        assert!(!CompanyName::new("").is_microsoft());
    }

    /// A name that merely mentions Microsoft is not Microsoft's. The prefix
    /// test exists to catch the bare `Microsoft` form, not to admit anything
    /// containing the word.
    #[test]
    fn a_vendor_that_only_mentions_microsoft_is_not_microsoft() {
        assert!(!CompanyName::new("Not Microsoft Ltd").is_microsoft());
        assert!(!CompanyName::new("Sogou (Microsoft partner)").is_microsoft());
    }

    /// A prefix test alone would accept a name that merely begins with the
    /// letters, so the match has to stop at a word boundary.
    #[test]
    fn a_vendor_whose_name_starts_with_the_letters_is_not_microsoft() {
        assert!(!CompanyName::new("MicrosoftFoo Ltd").is_microsoft());
        assert!(!CompanyName::new("Microsofty").is_microsoft());
    }

    /// Runs against whatever the test host has. A system DLL always carries a
    /// version resource naming Microsoft, so this covers the real FFI path.
    #[cfg(windows)]
    #[test]
    fn a_system_module_reports_microsoft() {
        let system32 = std::path::PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot is always set"))
            .join("System32")
            .join("kernel32.dll");

        let info = read(&system32).expect("kernel32 carries a version resource");

        assert!(info.company.expect("kernel32 names a company").is_microsoft());
        assert!(info.product.is_some());
    }

    #[cfg(windows)]
    #[test]
    fn a_file_with_no_version_resource_reports_nothing() {
        let file = tempfile::NamedTempFile::new().expect("tempfile");

        assert_eq!(read(file.path()), None);
    }
}
