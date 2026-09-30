//! Embeds the constants a replay's battle results are decoded through when
//! nothing newer has been fetched, and the Windows executable resources the
//! shell reads: the icon shown in the taskbar, on the window, and in Explorer.
//!
//! gpui embeds the application manifest itself from its own build script, so
//! this adds the icon only; a second manifest in the same binary is a duplicate
//! resource the linker refuses.
//!
//! winresource discovers `rc.exe` from host state, which a hermetic action
//! cannot do, so the Buck build compiles `assets/wows_toolkit_gpui.rc` with the
//! pinned `rc.exe` instead (see `GUI_LINK_FLAGS` in
//! `crates/wows-toolkit-gpui/BUCK`) and this is skipped there.
/// Copies one embedded resource into `OUT_DIR`, from the Buck-supplied location
/// where there is one and from the source tree otherwise.
fn copy_embedded_file(env_name: &str, fallback: &str, destination: &str) {
    println!("cargo:rerun-if-env-changed={env_name}");
    let source = std::env::var_os(env_name).map(std::path::PathBuf::from).unwrap_or_else(|| fallback.into());
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::copy(source, out_dir.join(destination)).expect("copy embedded file");
}

fn main() {
    copy_embedded_file("EMBEDDED_CONSTANTS", "../../embedded_resources/constants.json", "constants.json");
    println!("cargo:rerun-if-changed=../../assets/wows_toolkit.ico");

    // `windows` gates on the host, matching the build-dependency: winresource is
    // absent off Windows, so the reference to it must be too. The target check is
    // separate, for a Windows host building for another target.
    #[cfg(all(not(wows_buck_build), windows))]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/wows_toolkit.ico");
        resource.compile().expect("the Windows resources could not be compiled");
    }
}
