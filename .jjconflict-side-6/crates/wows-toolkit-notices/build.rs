//! Copies the repository's LICENSE and THIRD_PARTY_NOTICES.txt beside the crate's other generated
//! sources, so the library embeds them under Cargo and Buck alike.

use std::path::PathBuf;

fn main() {
    copy_embedded_file("PROJECT_LICENSE", "../../LICENSE", "LICENSE");
    copy_embedded_file("THIRD_PARTY_NOTICES", "../../THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_NOTICES.txt");
}

/// Buck names the file through `env_name`; Cargo reads it from the source tree.
fn copy_embedded_file(env_name: &str, cargo_path: &str, file_name: &str) {
    println!("cargo:rerun-if-env-changed={env_name}");
    println!("cargo:rerun-if-changed={cargo_path}");

    let source = std::env::var_os(env_name).map(PathBuf::from).unwrap_or_else(|| cargo_path.into());
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::copy(&source, out_dir.join(file_name))
        .unwrap_or_else(|error| panic!("copy {} to OUT_DIR: {error}", source.display()));
}
