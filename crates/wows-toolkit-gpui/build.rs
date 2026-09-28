//! Embeds the Windows executable resources the shell reads: the icon shown in
//! the taskbar, on the window, and in Explorer.
//!
//! gpui embeds the application manifest itself from its own build script, so
//! this adds the icon only; a second manifest in the same binary is a duplicate
//! resource the linker refuses.
//!
//! winresource discovers `rc.exe` from host state, which a hermetic action
//! cannot do, so the Buck build compiles `assets/wows_toolkit_gpui.rc` with the
//! pinned `rc.exe` instead (see `GUI_LINK_FLAGS` in
//! `crates/wows-toolkit-gpui/BUCK`) and this is skipped there.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/wows_toolkit.ico");

    #[cfg(not(wows_buck_build))]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/wows_toolkit.ico");
        resource.compile().expect("the Windows resources could not be compiled");
    }
}
