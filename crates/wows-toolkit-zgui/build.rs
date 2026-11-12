//! Embeds the Windows executable resources the shell reads: the icon shown in the taskbar, on the
//! window, and in Explorer.
//!
//! winresource discovers `rc.exe` from host state, which a hermetic action cannot do, so the Buck
//! build links `toolchains//windows:app_resource` instead and this is skipped there.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/wows_toolkit.ico");

    // `windows` gates on the host, matching the build-dependency. The target check is separate,
    // for a Windows host building for another target.
    #[cfg(all(not(wows_buck_build), windows))]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/wows_toolkit.ico");
        resource.compile().expect("the Windows resources could not be compiled");
    }
}
