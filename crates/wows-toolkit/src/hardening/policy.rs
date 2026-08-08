//! `SetProcessMitigationPolicy` wrappers.
//!
//! Each policy is irreversible for the life of the process, so these are called
//! once from `main` and never again. None of them is fatal: a kernel that
//! refuses a policy leaves the process exactly as unhardened as it was before
//! the call, which is worth reporting but never worth refusing to start over.
//!
//! The whole module is Windows-only. There is no cross-platform stub, so a
//! caller that forgets to gate the call fails to compile rather than reaching a
//! function that silently does nothing.
#![cfg(windows)]

use crate::util::win32::Win32Status;

/// Block AppInit DLLs, global `SetWindowsHookEx` hooks, Winsock layered service
/// providers, and legacy IMEs.
///
/// Local hooks keep working, so in-process UI automation is unaffected. This
/// must precede window creation, since the extension points attach at that
/// point.
pub fn disable_extension_points() -> Result<(), Win32Status> {
    use windows_sys::Win32::System::SystemServices::PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY;
    use windows_sys::Win32::System::Threading::ProcessExtensionPointDisablePolicy;

    /// `DisableExtensionPoints`, bit 0 of the policy's flags.
    const DISABLE_EXTENSION_POINTS: u32 = 1 << 0;

    let mut policy = PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY::default();
    policy.Anonymous.Flags = DISABLE_EXTENSION_POINTS;
    apply(ProcessExtensionPointDisablePolicy, &policy)
}

/// Refuse images loaded over the network, images carrying a low mandatory
/// label, and prefer System32 for any module named without a full path.
///
/// `NoLowMandatoryLabelImages` is what stops a DLL written by a low-integrity
/// process from being loaded here, and `PreferSystem32Images` closes the
/// search-order hijack where a module dropped next to the executable shadows a
/// system one.
pub fn restrict_image_loads() -> Result<(), Win32Status> {
    use windows_sys::Win32::System::SystemServices::PROCESS_MITIGATION_IMAGE_LOAD_POLICY;
    use windows_sys::Win32::System::Threading::ProcessImageLoadPolicy;

    const NO_REMOTE_IMAGES: u32 = 1 << 0;
    const NO_LOW_MANDATORY_LABEL_IMAGES: u32 = 1 << 1;
    const PREFER_SYSTEM32_IMAGES: u32 = 1 << 2;

    let mut policy = PROCESS_MITIGATION_IMAGE_LOAD_POLICY::default();
    policy.Anonymous.Flags = NO_REMOTE_IMAGES | NO_LOW_MANDATORY_LABEL_IMAGES | PREFER_SYSTEM32_IMAGES;
    apply(ProcessImageLoadPolicy, &policy)
}

/// Code Integrity Guard: load only images Microsoft signed.
///
/// The signature check accepts WHQL catalog signatures, which is what makes
/// this usable at all: the graphics driver keeps loading while the vendor's
/// separately signed overlay does not. Every injector this exists to stop is
/// plain Authenticode.
///
/// This cannot be unset, so the decision to call it belongs to the caller and
/// takes effect for the whole process lifetime.
pub fn require_microsoft_signed_images() -> Result<(), Win32Status> {
    use windows_sys::Win32::System::SystemServices::PROCESS_MITIGATION_BINARY_SIGNATURE_POLICY;
    use windows_sys::Win32::System::Threading::ProcessSignaturePolicy;

    /// `MicrosoftSignedOnly`, bit 0 of the policy's flags.
    const MICROSOFT_SIGNED_ONLY: u32 = 1 << 0;

    let mut policy = PROCESS_MITIGATION_BINARY_SIGNATURE_POLICY::default();
    policy.Anonymous.Flags = MICROSOFT_SIGNED_ONLY;
    apply(ProcessSignaturePolicy, &policy)
}

/// Stop Windows putting a modal dialog in front of the user when a policy
/// refuses to load an image.
///
/// A refused load is the policy working, but the loader raises it as a hard
/// error, which Windows presents as a "Bad Image" message box and which blocks
/// the process until someone dismisses it. Measured on the development machine:
/// with the signature policy in force, NVIDIA's capture DLL is refused with
/// `0xc0000428` and the resulting dialog stopped startup before the first frame.
/// Suppressing it turns a refused injection back into what it should be, which
/// is nothing the user has to see.
///
/// This is process-wide and, unlike the policies, is inherited by child
/// processes; `crate::hardening::prepare_child` is what keeps it from reaching
/// the game.
///
/// `SEM_FAILCRITICALERRORS` is the whole of it. `SEM_NOGPFAULTERRORBOX` is
/// deliberately not set, so Windows Error Reporting still sees a genuine crash.
/// The one other thing this suppresses is the no-media and corrupt-volume
/// prompt, so a replay read from a disconnected removable or network drive now
/// surfaces as an ordinary IO error instead of a system dialog.
pub fn suppress_image_load_dialogs() {
    use windows_sys::Win32::System::Diagnostics::Debug::GetErrorMode;
    use windows_sys::Win32::System::Diagnostics::Debug::SEM_FAILCRITICALERRORS;
    use windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode;

    // Read first rather than passing the flag alone: the mode is a process
    // setting that something else may already have contributed to, and
    // replacing it wholesale would drop whatever that was.
    // SAFETY: neither call has preconditions; both read and write a process
    // setting the OS synchronises internally.
    unsafe {
        SetErrorMode(GetErrorMode() | SEM_FAILCRITICALERRORS);
    }
}

fn apply<P>(
    kind: windows_sys::Win32::System::Threading::PROCESS_MITIGATION_POLICY,
    policy: &P,
) -> Result<(), Win32Status> {
    use windows_sys::Win32::System::Threading::SetProcessMitigationPolicy;

    // SAFETY: `policy` is a live reference to the policy struct this kind
    // expects, and the length is that struct's own size. The call reads the
    // buffer and does not retain it.
    let ok = unsafe { SetProcessMitigationPolicy(kind, std::ptr::from_ref(policy).cast(), std::mem::size_of::<P>()) };
    if ok != 0 { Ok(()) } else { Err(crate::util::win32::last_error()) }
}
