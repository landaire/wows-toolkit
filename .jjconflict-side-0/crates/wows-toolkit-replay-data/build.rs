//! Embeds the constants a replay's battle results are decoded through when
//! nothing newer has been fetched.

fn main() {
    const ENV_NAME: &str = "EMBEDDED_CONSTANTS";

    println!("cargo:rerun-if-env-changed={ENV_NAME}");
    println!("cargo:rerun-if-changed=../../embedded_resources/constants.json");

    // Buck supplies the file's location; Cargo reads it from the source tree.
    let source = std::env::var_os(ENV_NAME)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "../../embedded_resources/constants.json".into());
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::copy(source, out_dir.join("constants.json")).expect("copy the embedded constants");
}
