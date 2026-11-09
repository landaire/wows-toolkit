use std::path::PathBuf;

use nsh_bundler::{BundleOptions, bundle};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let entry = PathBuf::from(args.next().ok_or("missing TSX entry path")?);
    let output = PathBuf::from(args.next().ok_or("missing bundle output path")?);
    if args.next().is_some() {
        return Err("unexpected bundle tool argument".into());
    }

    let bundled = bundle(&BundleOptions {
        entry,
        minify: false,
        split: false,
        ui_sets: vec![nsh_base::registry().into(), wows_toolkit_nsh::registry().into()],
    })?;
    std::fs::write(output, bundled.code)?;
    Ok(())
}
