//! Inventory the modules loaded into this process that Windows did not ship.
//!
//! With the mitigations active this list should be empty, which makes a crash
//! report evidence that injection was not the cause. With them disabled it names
//! the culprit.
//!
//! Origin comes from the version resource, not from a signature check: the
//! kernel already made the trust decision by the time a module is resident, and
//! a company name is enough to identify where a module came from.

use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use super::version_info::CompanyName;
use super::version_info::ProductName;

/// A module resident in this process that did not come from Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignModule {
    pub path: PathBuf,
    pub company: Option<CompanyName>,
    pub product: Option<ProductName>,
}

impl std::fmt::Display for ForeignModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path.display())?;
        match (&self.company, &self.product) {
            (Some(company), Some(product)) => write!(f, " ({company}, {product})"),
            (Some(company), None) => write!(f, " ({company})"),
            (None, Some(product)) => write!(f, " ({product})"),
            (None, None) => write!(f, " (no version information)"),
        }
    }
}

/// Directories whose modules are not foreign.
///
/// The Windows directory covers System32, SysWOW64, WinSxS and the DriverStore,
/// which is where every WHQL graphics driver is staged. The application's own
/// directory covers this executable and anything shipped beside it.
#[derive(Debug, Clone, Default)]
pub struct OwnDirectories {
    pub windows: Option<PathBuf>,
    pub application: Option<PathBuf>,
}

/// Whether a loaded module came from somewhere other than Windows or this
/// application.
///
/// Comparison is case-insensitive because Windows paths are, and a module
/// reported as `C:\WINDOWS\System32\...` must match a `C:\Windows` root.
pub fn is_foreign(path: &Path, own: &OwnDirectories) -> bool {
    let module = path.to_string_lossy().to_lowercase();
    ![own.windows.as_deref(), own.application.as_deref()].into_iter().flatten().any(|root| {
        let mut root = root.to_string_lossy().to_lowercase();
        // Without a separator, a root of `C:\Windows` would also claim
        // `C:\WindowsApps`, which is where sideloaded packages live.
        if !root.ends_with('\\') {
            root.push('\\');
        }
        module.starts_with(&root)
    })
}

/// The directories whose modules this process considers its own.
pub fn own_directories() -> OwnDirectories {
    OwnDirectories {
        windows: std::env::var_os("SystemRoot").map(PathBuf::from),
        application: std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)),
    }
}

/// Every foreign module currently resident.
#[cfg(windows)]
pub fn foreign_modules() -> Inventory {
    let own = own_directories();
    Inventory::Taken(
        windows_modules::loaded_modules()
            .into_iter()
            .filter(|path| is_foreign(path, &own))
            .map(|path| {
                let info = super::version_info::read(&path).unwrap_or_default();
                ForeignModule { path, company: info.company, product: info.product }
            })
            .collect(),
    )
}

#[cfg(not(windows))]
pub fn foreign_modules() -> Inventory {
    Inventory::Unavailable
}

/// What the inventory found, or that it could not be taken.
///
/// Not a bare `Vec`: an empty list is a positive claim that nothing foreign was
/// resident, and a platform that cannot enumerate modules has not earned that
/// claim. A crash report that says "none" when nothing looked is worse than one
/// that admits it does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inventory {
    Taken(Vec<ForeignModule>),
    Unavailable,
}

static SNAPSHOT: OnceLock<Inventory> = OnceLock::new();

/// Take the inventory once, for the panic hook to report later.
///
/// The hook cannot take it itself. `K32EnumProcessModules` acquires the loader
/// lock, and the crash this feature exists to diagnose is a fault inside a
/// hooked graphics driver, where another thread may be holding that lock or
/// deadlocked under it. Enumerating there would turn a crash into a hang, after
/// the panic message and backtrace are already on disk but before the process
/// exits.
///
/// Call once the render stack is up: overlays hook at swapchain creation, so an
/// inventory taken in `main` would miss the modules most worth naming.
pub fn take_snapshot() {
    let _ = SNAPSHOT.set(foreign_modules());
}

/// The inventory taken at startup.
pub fn snapshot() -> Option<&'static Inventory> {
    SNAPSHOT.get()
}

/// The inventory as it appears in a crash report.
pub fn describe(inventory: &Inventory) -> String {
    let modules = match inventory {
        Inventory::Unavailable => {
            return "Loaded modules outside Windows and the application directory: not available on this platform\n"
                .to_string();
        }
        Inventory::Taken(modules) => modules,
    };

    if modules.is_empty() {
        return "Loaded modules outside Windows and the application directory: none\n".to_string();
    }

    let mut out = format!("Loaded modules outside Windows and the application directory: {}\n", modules.len());
    for module in modules {
        out.push_str(&format!("  {module}\n"));
    }
    out
}

#[cfg(windows)]
mod windows_modules {
    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::HMODULE;
    use windows_sys::Win32::System::ProcessStatus::K32EnumProcessModules;
    use windows_sys::Win32::System::ProcessStatus::K32GetModuleFileNameExW;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    /// Modules are enumerated twice: once to learn the count, once to read it.
    /// A module can load between the calls, so the second pass takes whatever
    /// the buffer held rather than trusting the first count.
    fn module_handles() -> Vec<HMODULE> {
        // SAFETY: GetCurrentProcess returns a pseudo-handle and cannot fail.
        let process = unsafe { GetCurrentProcess() };

        let mut needed: u32 = 0;
        // SAFETY: a zero-sized buffer asks only for the required byte count.
        let ok = unsafe { K32EnumProcessModules(process, std::ptr::null_mut(), 0, &mut needed) };
        if ok == 0 || needed == 0 {
            return Vec::new();
        }

        let capacity = needed as usize / std::mem::size_of::<HMODULE>();
        let mut handles: Vec<HMODULE> = vec![std::ptr::null_mut(); capacity];
        let mut written: u32 = 0;
        // SAFETY: `handles` has `needed` bytes of room, the size the call above
        // reported.
        let ok = unsafe { K32EnumProcessModules(process, handles.as_mut_ptr(), needed, &mut written) };
        if ok == 0 {
            return Vec::new();
        }

        // A concurrent load can make the second count exceed the buffer, in
        // which case only what fits was written.
        handles.truncate((written as usize / std::mem::size_of::<HMODULE>()).min(capacity));
        handles
    }

    fn module_path(module: HMODULE) -> Option<PathBuf> {
        // SAFETY: GetCurrentProcess returns a pseudo-handle and cannot fail.
        let process = unsafe { GetCurrentProcess() };
        // Long paths reach 32767 characters, but a module path that long would
        // be pathological; this covers every real one and truncation only costs
        // one line of a crash report.
        let mut buffer = [0u16; 1024];
        // SAFETY: `buffer` has the length passed, and `module` came from
        // K32EnumProcessModules on this same process.
        let length = unsafe { K32GetModuleFileNameExW(process, module, buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))
    }

    pub(super) fn loaded_modules() -> Vec<PathBuf> {
        module_handles().into_iter().filter_map(module_path).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own() -> OwnDirectories {
        OwnDirectories {
            windows: Some(PathBuf::from(r"C:\Windows")),
            application: Some(PathBuf::from(r"C:\Program Files\wows_toolkit")),
        }
    }

    #[test]
    fn system_modules_are_not_foreign() {
        for path in [
            r"C:\Windows\System32\kernel32.dll",
            r"C:\Windows\SysWOW64\kernel32.dll",
            r"C:\Windows\System32\DriverStore\FileRepository\nv_dispi.inf_amd64_x\nvoglv64.dll",
            r"C:\Windows\WinSxS\amd64_microsoft.windows.common-controls\comctl32.dll",
        ] {
            assert!(!is_foreign(Path::new(path), &own()), "{path} was reported as foreign");
        }
    }

    #[test]
    fn the_comparison_ignores_case_the_way_windows_does() {
        assert!(!is_foreign(Path::new(r"C:\WINDOWS\system32\NTDLL.DLL"), &own()));
    }

    #[test]
    fn injected_modules_are_foreign() {
        // Real paths come from K32GetModuleFileNameExW and are always
        // canonical, so the prefix test never has to reason about `..`.
        for path in [
            r"C:\Program Files\obs-studio\bin\64bit\graphics-hook64.dll",
            r"C:\Users\me\AppData\Local\Medal\medal-hook64.dll",
            r"C:\Program Files\NVIDIA Corporation\NvContainer\nvspcap64.dll",
        ] {
            assert!(is_foreign(Path::new(path), &own()), "{path} was not reported as foreign");
        }
    }

    /// The bug this guards: matching a bare prefix would treat `C:\WindowsApps`
    /// as part of `C:\Windows`, hiding every sideloaded package that injects.
    #[test]
    fn a_directory_that_merely_starts_with_the_windows_path_is_still_foreign() {
        let path = Path::new(r"C:\WindowsApps\SomeVendor.Overlay\hook.dll");

        assert!(is_foreign(path, &own()));
    }

    #[test]
    fn everything_is_foreign_when_no_directory_is_known() {
        assert!(is_foreign(Path::new(r"C:\Windows\System32\kernel32.dll"), &OwnDirectories::default()));
    }

    #[test]
    fn the_description_says_so_when_nothing_foreign_is_loaded() {
        assert!(describe(&Inventory::Taken(Vec::new())).contains("none"));
    }

    /// A crash report must not claim nothing foreign was loaded on the strength
    /// of never having looked.
    #[test]
    fn an_inventory_that_could_not_be_taken_does_not_claim_none() {
        let described = describe(&Inventory::Unavailable);

        assert!(described.contains("not available"));
        assert!(!described.contains("none"));
    }

    #[test]
    fn the_description_names_each_module_and_its_origin() {
        let described = describe(&Inventory::Taken(vec![ForeignModule {
            path: PathBuf::from(r"C:\obs\graphics-hook64.dll"),
            company: Some(CompanyName::new("OBS Project")),
            product: Some(ProductName::new("OBS Studio")),
        }]));

        assert!(described.contains("graphics-hook64.dll"));
        assert!(described.contains("OBS Project"));
        assert!(described.contains("OBS Studio"));
    }

    #[test]
    fn a_module_with_no_version_information_still_appears() {
        let described = describe(&Inventory::Taken(vec![ForeignModule {
            path: PathBuf::from(r"C:\mystery\hook.dll"),
            company: None,
            product: None,
        }]));

        assert!(described.contains("hook.dll"));
        assert!(described.contains("no version information"));
    }

    /// Runs against whatever the test host has injected, including nothing.
    #[cfg(windows)]
    #[test]
    fn enumerating_this_process_finds_its_own_windows_modules() {
        let own = own_directories();
        assert!(own.windows.is_some(), "SystemRoot is set on every Windows host");

        // Visible under --nocapture: this is the list a crash report would name.
        let Inventory::Taken(modules) = foreign_modules() else {
            panic!("module enumeration is available on Windows");
        };
        for module in modules {
            println!("{module}");
        }

        let all = windows_modules::loaded_modules();
        assert!(
            all.iter().any(|path| path.file_name().is_some_and(|name| name.eq_ignore_ascii_case("kernel32.dll"))),
            "kernel32 is resident in every process"
        );
    }
}
