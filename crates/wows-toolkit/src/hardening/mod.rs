//! Windows process mitigation policies.
//!
//! Third-party software injects DLLs into this process and hooks graphics APIs.
//! The observed result is deadlocks and crashes during rendering, usually
//! surfacing inside the graphics driver so they read as driver bugs. Pinning the
//! adapter closed the Vulkan implicit-layer route; these policies close the rest.
//!
//! Everything here is best effort. A policy the kernel refuses leaves the
//! process exactly as it was, which is worth recording and never worth refusing
//! to start over.

pub mod ime;
pub mod modules;
pub mod policy;
pub mod version_info;

use std::sync::OnceLock;

use crate::util::win32::Win32Status;

use ime::TextService;

/// Whether this launch applies process mitigations at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hardening {
    Apply,
    Skip(SkipReason),
}

/// Why a policy was not attempted.
///
/// A crash report has to distinguish these. A machine that demoted itself to
/// the unhardened render mode after four failed launches otherwise looks
/// identical to one where the user passed a flag, and those call for opposite
/// responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The render mode this launch resolved to leaves the process unhardened.
    RenderMode,
    /// `--no-hardening` was passed.
    CommandLine,
    /// The user turned it off.
    Setting,
    /// The setting could not be read, so the user's choice is unknown. Skipping
    /// is the safe direction: a policy applied against a preference that said
    /// otherwise cannot be undone for the life of the process.
    SettingUnreadable,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RenderMode => "the render mode does not harden",
            Self::CommandLine => "--no-hardening",
            Self::Setting => "turned off in settings",
            Self::SettingUnreadable => "the setting could not be read",
        }
    }
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the user asked for regarding Code Integrity Guard, the one policy with
/// a real compatibility cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeIntegrityPreference {
    /// Apply it unless a text service is registered that it would block.
    #[default]
    Automatic,
    Always,
    Never,
}

impl CodeIntegrityPreference {
    pub const ALL: [Self; 3] = [Self::Automatic, Self::Always, Self::Never];

    /// Translation key for this choice's label.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Automatic => "ui.settings.app.code_integrity_automatic",
            Self::Always => "ui.settings.app.code_integrity_always",
            Self::Never => "ui.settings.app.code_integrity_never",
        }
    }
}

/// What the caller decided before any policy was touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardeningRequest {
    pub hardening: Hardening,
    /// `None` when the setting could not be read, which is not the same as the
    /// user never having chosen one. Absence has a correct default; a failed
    /// read has none, because the value that could not be read may have been
    /// the one turning the policy off.
    pub code_integrity: Option<CodeIntegrityPreference>,
}

/// One of the policies this application applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mitigation {
    ExtensionPointDisable,
    ImageLoad,
    CodeIntegrityGuard,
}

impl Mitigation {
    pub const ALL: [Self; 3] = [Self::ExtensionPointDisable, Self::ImageLoad, Self::CodeIntegrityGuard];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExtensionPointDisable => "extension point disable",
            Self::ImageLoad => "image load restrictions",
            Self::CodeIntegrityGuard => "code integrity guard",
        }
    }
}

impl std::fmt::Display for Mitigation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What became of one policy.
///
/// A per-policy outcome rather than a bool, so a policy the text service probe
/// declined to apply stays distinguishable from one the kernel rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MitigationOutcome {
    Applied,
    /// Nothing was attempted, for the reason carried.
    SkippedByRequest(SkipReason),
    /// Applying it would have blocked a text service the user types with.
    SkippedForTextServices(Vec<TextService>),
    /// The text service probe failed, so nothing is known about what applying
    /// the policy would break.
    SkippedProbeFailed(ime::TextServiceError),
    /// The kernel refused it.
    Rejected(Win32Status),
    /// Not a Windows build.
    Unsupported,
}

impl std::fmt::Display for MitigationOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Applied => f.write_str("applied"),
            Self::SkippedByRequest(reason) => write!(f, "skipped ({reason})"),
            Self::SkippedForTextServices(services) => {
                write!(f, "skipped for {} text service(s):", services.len())?;
                for service in services {
                    write!(f, " {service};")?;
                }
                Ok(())
            }
            Self::SkippedProbeFailed(error) => write!(f, "skipped, the text service probe failed: {error}"),
            Self::Rejected(status) => write!(f, "rejected by the kernel ({status})"),
            Self::Unsupported => f.write_str("not supported on this platform"),
        }
    }
}

/// What each policy did this launch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MitigationReport {
    outcomes: Vec<(Mitigation, MitigationOutcome)>,
}

impl MitigationReport {
    fn record(&mut self, mitigation: Mitigation, outcome: MitigationOutcome) {
        self.outcomes.push((mitigation, outcome));
    }

    pub fn entries(&self) -> impl Iterator<Item = (Mitigation, &MitigationOutcome)> {
        self.outcomes.iter().map(|(mitigation, outcome)| (*mitigation, outcome))
    }

    pub fn outcome(&self, mitigation: Mitigation) -> Option<&MitigationOutcome> {
        self.outcomes.iter().find(|(kind, _)| *kind == mitigation).map(|(_, outcome)| outcome)
    }
}

impl std::fmt::Display for MitigationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.outcomes.is_empty() {
            return f.write_str("Process mitigations: none attempted\n");
        }
        f.write_str("Process mitigations:\n")?;
        for (mitigation, outcome) in &self.outcomes {
            writeln!(f, "  {mitigation}: {outcome}")?;
        }
        Ok(())
    }
}

/// Keep this process's hardening from reaching a child.
///
/// Two things leak into a child otherwise, and both matter most for the game:
/// the Vulkan driver pin and layer suppression travel in the environment block
/// that `CreateProcess` copies, and the suppressed error mode is inherited
/// outright. Opening a replay must not silently move World of Warships onto
/// whichever adapter this app descended to, strip its Steam overlay, or change
/// how it reports its own load failures. The updater relaunch wants the same
/// treatment, where an inherited pin would outlive the mode that set it.
///
/// The mitigation policies themselves are not inherited, so nothing here has to
/// undo them.
pub fn prepare_child(command: &mut std::process::Command) -> &mut std::process::Command {
    for name in crate::gpu::PIN_VARS {
        command.env_remove(name);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        use windows_sys::Win32::System::Threading::CREATE_DEFAULT_ERROR_MODE;

        // Without this the child starts with our suppressed error mode, so a
        // genuine load failure in the game would fail silently instead of
        // telling the user what broke.
        command.creation_flags(CREATE_DEFAULT_ERROR_MODE);
    }
    command
}

static REPORT: OnceLock<MitigationReport> = OnceLock::new();

/// What the mitigations did, once they have been applied.
///
/// `main` runs before the tracing subscriber exists, so the report is stashed
/// here and read back by the app and by the panic hook.
pub fn report() -> Option<&'static MitigationReport> {
    REPORT.get()
}

/// Apply the process mitigations and record what each one did.
///
/// Called once, from `main`, after the render configuration is resolved and
/// before any window exists: the extension points attach at window creation and
/// overlays hook at swapchain creation, so anything later is too late.
pub fn apply_startup_mitigations(request: HardeningRequest) -> MitigationReport {
    let report = build_report(request);
    // A second call would mean a second attempt at policies that cannot be
    // applied twice, so the first report is the one that stands.
    let _ = REPORT.set(report.clone());
    report
}

/// The skip decision is platform-independent so that a launch which asked for
/// no hardening reports that on every platform, rather than reporting the
/// platform's own lack of support and losing why nothing was attempted.
fn build_report(request: HardeningRequest) -> MitigationReport {
    let mut report = MitigationReport::default();
    match request.hardening {
        Hardening::Skip(reason) => {
            for mitigation in Mitigation::ALL {
                report.record(mitigation, MitigationOutcome::SkippedByRequest(reason));
            }
        }
        Hardening::Apply => apply_policies(request.code_integrity, &mut report),
    }
    report
}

#[cfg(windows)]
fn apply_policies(code_integrity: Option<CodeIntegrityPreference>, report: &mut MitigationReport) {
    // Before any policy can refuse a load, so the first refusal is silent
    // rather than a modal dialog the user has to dismiss.
    policy::suppress_image_load_dialogs();

    report.record(Mitigation::ExtensionPointDisable, outcome(policy::disable_extension_points()));
    report.record(Mitigation::ImageLoad, outcome(policy::restrict_image_loads()));
    report.record(Mitigation::CodeIntegrityGuard, code_integrity_outcome(code_integrity));
}

#[cfg(not(windows))]
fn apply_policies(_code_integrity: Option<CodeIntegrityPreference>, report: &mut MitigationReport) {
    for mitigation in Mitigation::ALL {
        report.record(mitigation, MitigationOutcome::Unsupported);
    }
}

#[cfg(windows)]
fn outcome(result: Result<(), Win32Status>) -> MitigationOutcome {
    match result {
        Ok(()) => MitigationOutcome::Applied,
        Err(status) => MitigationOutcome::Rejected(status),
    }
}

#[cfg(windows)]
fn code_integrity_outcome(preference: Option<CodeIntegrityPreference>) -> MitigationOutcome {
    match preference {
        None => return MitigationOutcome::SkippedByRequest(SkipReason::SettingUnreadable),
        Some(CodeIntegrityPreference::Never) => {
            return MitigationOutcome::SkippedByRequest(SkipReason::Setting);
        }
        Some(CodeIntegrityPreference::Always) => return outcome(policy::require_microsoft_signed_images()),
        Some(CodeIntegrityPreference::Automatic) => {}
    }

    // A probe that cannot read the registry proves nothing about what is
    // installed, and applying the policy on a guess risks leaving someone
    // unable to type. Treat it the way a found text service is treated.
    let blocked = match ime::blocked_services() {
        Ok(services) => services,
        Err(error) => return MitigationOutcome::SkippedProbeFailed(error),
    };

    if blocked.is_empty() {
        outcome(policy::require_microsoft_signed_images())
    } else {
        MitigationOutcome::SkippedForTextServices(blocked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skipped(reason: SkipReason) -> MitigationReport {
        build_report(HardeningRequest { hardening: Hardening::Skip(reason), code_integrity: Default::default() })
    }

    #[test]
    fn a_skipped_request_records_every_policy_as_skipped() {
        let report = skipped(SkipReason::CommandLine);

        for mitigation in Mitigation::ALL {
            assert_eq!(report.outcome(mitigation), Some(&MitigationOutcome::SkippedByRequest(SkipReason::CommandLine)));
        }
    }

    #[test]
    fn every_policy_appears_in_the_report() {
        assert_eq!(skipped(SkipReason::CommandLine).entries().count(), Mitigation::ALL.len());
    }

    #[test]
    fn a_skipped_report_reads_as_skipped_rather_than_as_a_failure() {
        let described = skipped(SkipReason::CommandLine).to_string();

        assert!(described.contains("skipped"));
        assert!(!described.contains("rejected"));
    }

    /// The reasons must stay distinguishable in a crash report: a machine that
    /// demoted itself to the unhardened render mode needs a different answer
    /// from one where the user passed a flag.
    #[test]
    fn each_skip_reason_reads_differently() {
        let descriptions: Vec<String> =
            [SkipReason::RenderMode, SkipReason::CommandLine, SkipReason::Setting, SkipReason::SettingUnreadable]
                .into_iter()
                .map(|reason| skipped(reason).to_string())
                .collect();

        for (index, described) in descriptions.iter().enumerate() {
            assert!(
                descriptions.iter().enumerate().all(|(other, text)| other == index || text != described),
                "two skip reasons produced the same report"
            );
        }
    }

    #[test]
    fn an_empty_report_says_nothing_was_attempted() {
        assert!(MitigationReport::default().to_string().contains("none attempted"));
    }

    /// An unreadable setting must not be treated as an unset one. The default
    /// is `Automatic`, which applies the policy, so collapsing a failed read
    /// into it would apply a policy the user may have turned off, irreversibly.
    #[cfg(windows)]
    #[test]
    fn an_unreadable_setting_skips_rather_than_falling_back_to_the_default() {
        assert_eq!(code_integrity_outcome(None), MitigationOutcome::SkippedByRequest(SkipReason::SettingUnreadable));
    }

    #[cfg(windows)]
    #[test]
    fn turning_the_policy_off_in_settings_is_reported_as_such() {
        assert_eq!(
            code_integrity_outcome(Some(CodeIntegrityPreference::Never)),
            MitigationOutcome::SkippedByRequest(SkipReason::Setting)
        );
    }

    #[test]
    fn a_rejection_reports_the_status_it_failed_with() {
        let mut report = MitigationReport::default();
        report.record(Mitigation::CodeIntegrityGuard, MitigationOutcome::Rejected(Win32Status::new(87)));

        assert!(report.to_string().contains("87"));
    }

    #[test]
    fn a_text_service_skip_names_the_service_that_caused_it() {
        let mut report = MitigationReport::default();
        report.record(
            Mitigation::CodeIntegrityGuard,
            MitigationOutcome::SkippedForTextServices(vec![TextService {
                clsid: ime::TextServiceClsid::new("{sogou}"),
                module: std::path::PathBuf::from(r"C:\sogou\tip.dll"),
                origin: ime::ServiceOrigin::ThirdParty(version_info::CompanyName::new("Sogou.com Inc.")),
            }]),
        );

        let described = report.to_string();
        assert!(described.contains("Sogou.com Inc."));
        assert!(described.contains("tip.dll"));
    }
}
