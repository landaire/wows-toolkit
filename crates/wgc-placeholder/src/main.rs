//! Installed as `wgc.exe` in the launcher's Wine prefix. `wgc_api.dll` reads
//! only its version resource (wgc.rc) and treats WGC as outdated without one.
//! Nothing runs it on purpose; if something does, it exits at once.
//!
//! The Buck build links neither std nor the C runtime, which leaves little
//! beyond the PE headers and the version resource. It sets panic=abort and the
//! entry point (BUCK).

#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(all(wows_buck_build, windows), no_std, no_main)]

#[cfg(all(wows_buck_build, windows))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(all(wows_buck_build, windows))]
#[unsafe(no_mangle)]
extern "system" fn wgc_placeholder_start() -> u32 {
    0
}

#[cfg(not(all(wows_buck_build, windows)))]
fn main() {}
