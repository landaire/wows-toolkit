//! Prove Code Integrity Guard does what the design claims: admit Windows, and
//! reject an image Microsoft did not sign.
//!
//! This lives in its own test binary because the policy is irreversible and
//! process-wide. Once it is applied every later image load in this process is
//! subject to it, so it cannot share a process with tests that do not expect
//! that. The single test below is deliberately the only one here.

#![cfg(windows)]

use std::path::Path;
use std::path::PathBuf;

use windows_sys::Win32::Foundation::ERROR_INVALID_IMAGE_HASH;
use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
use windows_sys::Win32::System::SystemServices::PROCESS_MITIGATION_BINARY_SIGNATURE_POLICY;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::GetProcessMitigationPolicy;
use windows_sys::Win32::System::Threading::ProcessSignaturePolicy;

use wows_toolkit::hardening::policy;

/// A load attempt, and the error it failed with.
enum LoadResult {
    Loaded(HMODULE),
    Failed(u32),
}

fn load(path: &Path) -> LoadResult {
    let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call.
    let module = unsafe { LoadLibraryW(wide.as_ptr()) };
    if module.is_null() {
        // SAFETY: reads thread-local state set by the failed call above.
        LoadResult::Failed(unsafe { GetLastError() })
    } else {
        LoadResult::Loaded(module)
    }
}

fn release(module: HMODULE) {
    // SAFETY: `module` came from a successful LoadLibraryW and is released once.
    unsafe { FreeLibrary(module) };
}

/// Whether the signature policy is in force, read back from the kernel rather
/// than inferred from the call having returned success.
fn microsoft_signed_only() -> bool {
    let mut policy = PROCESS_MITIGATION_BINARY_SIGNATURE_POLICY::default();
    // SAFETY: the buffer is the struct this policy kind expects and the length
    // is its own size; GetCurrentProcess returns a pseudo-handle.
    let ok = unsafe {
        GetProcessMitigationPolicy(
            GetCurrentProcess(),
            ProcessSignaturePolicy,
            std::ptr::from_mut(&mut policy).cast(),
            std::mem::size_of_val(&policy),
        )
    };
    // SAFETY: the union's `Flags` member aliases the bitfield and is always
    // initialized by a successful read.
    ok != 0 && unsafe { policy.Anonymous.Flags } & 1 == 1
}

/// A byte-identical copy of a signed system DLL keeps its catalog signature,
/// because a catalog binds a file hash and a copy hashes the same. Changing a
/// byte in the DOS stub breaks that binding without touching anything the
/// loader executes: the stub is dead weight on Win32 and is never run.
fn unsigned_copy_of_a_system_dll(directory: &Path) -> PathBuf {
    let source = PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot is set on every Windows host"))
        .join("System32")
        .join("version.dll");
    let mut image = std::fs::read(&source).expect("a system DLL is readable");

    // Offset 0x40 is inside the DOS stub's "This program cannot be run in DOS
    // mode" text, well before the PE header at e_lfanew.
    const DOS_STUB_TEXT: usize = 0x40;
    assert!(image.len() > DOS_STUB_TEXT, "the system DLL is too small to be a PE image");
    image[DOS_STUB_TEXT] ^= 0xff;

    let target = directory.join("tampered_version.dll");
    std::fs::write(&target, image).expect("the temp directory is writable");
    target
}

#[test]
fn code_integrity_guard_admits_windows_and_rejects_what_microsoft_did_not_sign() {
    let directory = tempfile::tempdir().expect("tempdir");
    let tampered = unsigned_copy_of_a_system_dll(directory.path());

    // Establish that the tampered image is loadable at all, so the rejection
    // below is attributable to the policy and not to a broken PE. It is freed
    // immediately: a module that stayed resident would make the second attempt
    // a refcount bump that succeeds regardless of any policy.
    match load(&tampered) {
        LoadResult::Loaded(module) => release(module),
        LoadResult::Failed(code) => panic!("the tampered image was not loadable before the policy: win32 {code}"),
    }

    assert!(!microsoft_signed_only(), "the policy was already in force before this test applied it");
    policy::require_microsoft_signed_images().expect("the signature policy is supported on every supported Windows");
    assert!(microsoft_signed_only(), "the policy call reported success but the kernel does not have it in force");

    // A Windows component must still load. Without this the test would pass on
    // a policy that blocks everything, which would take the application with it.
    let system32 =
        PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot is set")).join("System32").join("winmm.dll");
    match load(&system32) {
        LoadResult::Loaded(module) => release(module),
        LoadResult::Failed(code) => panic!("a Microsoft-signed system DLL was blocked: win32 {code}"),
    }

    match load(&tampered) {
        LoadResult::Loaded(module) => {
            release(module);
            panic!("an image Microsoft did not sign loaded with the signature policy in force");
        }
        LoadResult::Failed(code) => assert_eq!(
            code, ERROR_INVALID_IMAGE_HASH,
            "the load was refused, but for a reason other than the signature policy"
        ),
    }
}
