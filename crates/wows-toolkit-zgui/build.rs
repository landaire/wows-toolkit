//! Places the application icon in `OUT_DIR` for `app_icon`, and embeds the Windows executable
//! resources Explorer reads.
//!
//! Buck names the icon through `WOWS_TOOLKIT_ICON`, because a hermetic build script cannot reach
//! outside its package. winresource discovers `rc.exe` from host state, which a hermetic action
//! cannot do either, so the Buck build links `toolchains//windows:app_resource` instead and skips it
//! here.
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=WOWS_TOOLKIT_ICON");
    let icon = std::env::var_os("WOWS_TOOLKIT_ICON")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../assets/wows_toolkit_256.png"));
    println!("cargo:rerun-if-changed={}", icon.display());
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR for a build script"));
    std::fs::copy(&icon, out_dir.join("wows_toolkit_256.png")).expect("the application icon could not be copied");

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
