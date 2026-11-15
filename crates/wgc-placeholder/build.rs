// Cargo only. Buck compiles wgc.rc with the cross toolchain's windres (BUCK).
fn main() {
    println!("cargo:rerun-if-changed=wgc.rc");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_resource_file("wgc.rc");
    resource.compile().expect("compile wgc.rc");
}
