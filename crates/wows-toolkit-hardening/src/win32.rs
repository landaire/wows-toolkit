//! Win32 status codes.

/// A Win32 status code, as returned by `GetLastError` and by the registry
/// functions, which report status through their return value rather than
/// through `GetLastError`.
///
/// A newtype because these travel through error enums alongside other numbers
/// and are meaningless without knowing which space they belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Win32Status(u32);

impl Win32Status {
    pub fn new(code: u32) -> Self {
        Self(code)
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for Win32Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "win32 status {}", self.0)
    }
}

/// The calling thread's last error.
#[cfg(windows)]
pub fn last_error() -> Win32Status {
    // SAFETY: GetLastError reads thread-local state and has no preconditions.
    Win32Status(unsafe { windows_sys::Win32::Foundation::GetLastError() })
}
