fn main() {
    let opt_level = std::env::var("OPT_LEVEL").expect("OPT_LEVEL is missing");
    println!("cargo:rustc-env=BUILD_SCRIPT_OPT_LEVEL={opt_level}");
}
