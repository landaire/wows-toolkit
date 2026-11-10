//! Bundles the TSX interface into `OUT_DIR/bundle.js` and names it in
//! `NSH_BUNDLE_PATH`, which `main.rs` embeds.

use std::path::Path;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is not set")?);
    let ui = manifest_dir.join("../wows-toolkit-nsh/ui");
    watch(&ui)?;

    let bundled = nsh_bundler::bundle(&nsh_bundler::BundleOptions {
        entry: ui.join("App.tsx"),
        minify: false,
        split: false,
        ui_sets: vec![nsh_base::registry().into(), wows_toolkit_nsh::registry().into()],
    })?;
    let output = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?).join("bundle.js");
    std::fs::write(&output, bundled.code)?;
    println!("cargo:rustc-env=NSH_BUNDLE_PATH={}", output.display());
    Ok(())
}

/// Reruns the bundle when any file under `dir` changes.
fn watch(dir: &Path) -> std::io::Result<()> {
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            watch(&path)?;
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    Ok(())
}
