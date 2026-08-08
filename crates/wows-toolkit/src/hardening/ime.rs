//! Find text services that Code Integrity Guard would block.
//!
//! A TSF text service loads its DLL into every process the user types into. If
//! CIG blocks one, that user cannot enter text in this application at all, and
//! the policy cannot be lifted once applied. The game has a large CJK player
//! base and this application is localized, so that population is real and the
//! probe is what keeps the policy off their machines.
//!
//! Classification is by the version resource's `CompanyName`, not by where the
//! DLL lives. Windows itself registers text services outside `%SystemRoot%`
//! (`tiptsf.dll` under Common Files, `TableTextService.dll` under Program
//! Files), so a path test would report a third-party service on a clean install
//! and silently disable the policy everywhere.

use std::path::PathBuf;

use thiserror::Error;

use super::version_info::CompanyName;

/// Registry key holding one subkey per registered text input processor.
#[cfg(windows)]
const TIP_KEY: &str = r"SOFTWARE\Microsoft\CTF\TIP";

/// The COM class id a text service is registered under.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextServiceClsid(String);

impl TextServiceClsid {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TextServiceClsid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Who shipped a text service, as far as its own version resource says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceOrigin {
    Microsoft,
    ThirdParty(CompanyName),
    /// The DLL carries no version resource naming anyone.
    Unidentified,
}

/// A text service that loads a DLL into this process.
///
/// Out-of-process (`LocalServer32`) registrations and stale entries that resolve
/// to no server at all are not represented: neither can load anything here, so
/// neither is evidence for or against the policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextService {
    pub clsid: TextServiceClsid,
    pub module: PathBuf,
    pub origin: ServiceOrigin,
}

impl TextService {
    /// Whether Code Integrity Guard would stop this service from loading.
    ///
    /// An unidentified service counts. Something that will load into this
    /// process and cannot be attributed is exactly the case not worth breaking,
    /// and the cost of being wrong here is only that the policy is skipped,
    /// which is the behaviour that already exists.
    pub fn blocked_by_code_integrity(&self) -> bool {
        match self.origin {
            ServiceOrigin::Microsoft => false,
            ServiceOrigin::ThirdParty(_) | ServiceOrigin::Unidentified => true,
        }
    }
}

impl std::fmt::Display for TextService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.origin {
            ServiceOrigin::Microsoft => write!(f, "{} (Microsoft)", self.module.display()),
            ServiceOrigin::ThirdParty(company) => write!(f, "{} ({company})", self.module.display()),
            ServiceOrigin::Unidentified => write!(f, "{} (unidentified)", self.module.display()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TextServiceError {
    #[error("failed to open registry key {key}: {status}")]
    OpenKey { key: String, status: crate::util::win32::Win32Status },
    #[error("failed to enumerate subkeys of {key}: {status}")]
    EnumerateKeys { key: String, status: crate::util::win32::Win32Status },
}

/// Classify a service from what its version resource claims.
pub fn classify(company: Option<CompanyName>) -> ServiceOrigin {
    match company {
        Some(company) if company.is_microsoft() => ServiceOrigin::Microsoft,
        Some(company) => ServiceOrigin::ThirdParty(company),
        None => ServiceOrigin::Unidentified,
    }
}

/// Every in-process text service registered on this machine.
#[cfg(windows)]
pub fn probe() -> Result<Vec<TextService>, TextServiceError> {
    windows_ime::probe()
}

#[cfg(not(windows))]
pub fn probe() -> Result<Vec<TextService>, TextServiceError> {
    Ok(Vec::new())
}

/// The services that would stop working under Code Integrity Guard.
pub fn blocked_services() -> Result<Vec<TextService>, TextServiceError> {
    Ok(probe()?.into_iter().filter(TextService::blocked_by_code_integrity).collect())
}

#[cfg(windows)]
mod windows_ime {
    use std::path::PathBuf;

    use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;

    use crate::util::registry::HKEY_LOCAL_MACHINE;
    use crate::util::registry::RegKey;

    use super::TIP_KEY;
    use super::TextService;
    use super::TextServiceClsid;
    use super::TextServiceError;
    use super::classify;

    /// The DLL a class id loads in-process, or `None` when it names an
    /// out-of-process server or is not registered at all.
    ///
    /// Per-user class registrations shadow machine-wide ones, so `HKCU` is
    /// consulted first, the same order the merged `HKEY_CLASSES_ROOT` view uses.
    fn inproc_server(clsid: &str) -> Option<PathBuf> {
        for parent in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            let key = format!(r"SOFTWARE\Classes\CLSID\{clsid}\InprocServer32");
            let Ok(server) = RegKey::open(parent, &key) else {
                continue;
            };
            // The default value holds the module path; an entry with the key but
            // no default value names nothing loadable.
            let Some(module) = server.query_strings("").into_iter().next() else {
                continue;
            };
            let module = PathBuf::from(module);
            // A registration left behind by an uninstall names a DLL that is no
            // longer there. It cannot load, so it cannot be blocked, and
            // treating it as an unidentifiable service would let one piece of
            // stale registry turn the policy off on this machine for good.
            if module.exists() {
                return Some(module);
            }
        }
        None
    }

    /// Class ids registered as text services under one root.
    fn registered_clsids(parent: windows_sys::Win32::System::Registry::HKEY) -> Result<Vec<String>, TextServiceError> {
        let key = RegKey::open(parent, TIP_KEY)
            .map_err(|status| TextServiceError::OpenKey { key: TIP_KEY.to_string(), status })?;
        key.subkey_names().map_err(|status| TextServiceError::EnumerateKeys { key: TIP_KEY.to_string(), status })
    }

    pub(super) fn probe() -> Result<Vec<TextService>, TextServiceError> {
        // Text services are registered per machine and per user, and a per-user
        // install is how an IME arrives on a machine the user cannot
        // administer. Reading only the machine-wide key would miss it and then
        // apply a policy that blocks it.
        //
        // The per-user key is absent on a user who has installed none, which is
        // an absence rather than a failure. The machine-wide key exists on every
        // Windows install, so failing to read that one is a real probe failure
        // and the caller must not conclude anything from an empty result.
        let per_user = registered_clsids(HKEY_CURRENT_USER).unwrap_or_default();
        let machine_wide = registered_clsids(HKEY_LOCAL_MACHINE)?;

        let mut services = Vec::new();
        let mut seen = std::collections::HashSet::new();
        // Per-user first: a class id registered under both roots resolves to
        // the per-user registration.
        for name in per_user.into_iter().chain(machine_wide) {
            if !seen.insert(name.to_lowercase()) {
                continue;
            }
            let Some(module) = inproc_server(&name) else {
                continue;
            };
            let company = crate::hardening::version_info::read(&module).and_then(|info| info.company);
            services.push(TextService { clsid: TextServiceClsid::new(name), origin: classify(company), module });
        }

        Ok(services)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(origin: ServiceOrigin) -> TextService {
        TextService { clsid: TextServiceClsid::new("{test}"), module: PathBuf::from(r"C:\ime\tip.dll"), origin }
    }

    #[test]
    fn a_microsoft_service_keeps_working_under_the_policy() {
        assert_eq!(classify(Some(CompanyName::new("Microsoft Corporation"))), ServiceOrigin::Microsoft);
        assert!(!service(ServiceOrigin::Microsoft).blocked_by_code_integrity());
    }

    #[test]
    fn a_third_party_service_would_be_blocked() {
        let origin = classify(Some(CompanyName::new("Sogou.com Inc.")));

        assert_eq!(origin, ServiceOrigin::ThirdParty(CompanyName::new("Sogou.com Inc.")));
        assert!(service(origin).blocked_by_code_integrity());
    }

    #[test]
    fn a_service_that_names_nobody_counts_as_blocked() {
        assert_eq!(classify(None), ServiceOrigin::Unidentified);
        assert!(service(ServiceOrigin::Unidentified).blocked_by_code_integrity());
    }

    /// The bug this guards: Windows registers text services outside
    /// `%SystemRoot%`, so classifying by path would report a third-party
    /// service on a clean install and disable the policy on every machine.
    #[test]
    fn microsoft_services_outside_the_windows_directory_are_still_microsoft() {
        for module in [
            r"C:\Program Files\Common Files\microsoft shared\ink\tiptsf.dll",
            r"C:\Program Files\Windows NT\TableTextService\TableTextService.dll",
        ] {
            let service = TextService {
                clsid: TextServiceClsid::new("{test}"),
                module: PathBuf::from(module),
                origin: classify(Some(CompanyName::new("Microsoft Corporation"))),
            };

            assert!(!service.blocked_by_code_integrity(), "{module} was treated as third party");
        }
    }

    /// Runs against whatever the test host has registered, including nothing.
    #[cfg(windows)]
    #[test]
    fn probing_real_text_services_yields_only_in_process_ones() {
        let services = probe().expect("the CTF TIP key is readable on every supported Windows");

        // Visible under --nocapture, which is the only way to see what this
        // machine registers without a registry dump.
        for service in &services {
            println!("{} {}", service.clsid, service);
        }

        for service in &services {
            assert!(!service.module.as_os_str().is_empty());
        }
    }
}
