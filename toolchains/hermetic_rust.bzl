load("@prelude//rust:rust_toolchain.bzl", "PanicRuntime", "RustToolchainInfo")

def _hermetic_rust_toolchain_impl(ctx):
    root = ctx.attrs.nix_toolchain_root
    return [
        DefaultInfo(),
        RustToolchainInfo(
            allow_lints = [],
            clippy_driver = RunInfo(args = [root + "/bin/clippy-driver"]),
            clippy_toml = None,
            compiler = RunInfo(args = [root + "/bin/rustc"]),
            default_edition = ctx.attrs.default_edition,
            deny_lints = [],
            doctests = False,
            nightly_features = False,
            panic_runtime = PanicRuntime("unwind"),
            report_unused_deps = False,
            rustc_binary_flags = [],
            rustc_flags = [],
            rustc_target_triple = ctx.attrs.rustc_target_triple,
            rustc_test_flags = [],
            rustdoc = RunInfo(args = [root + "/bin/rustdoc"]),
            rustdoc_flags = [],
            warn_lints = [],
        ),
    ]

hermetic_rust_toolchain = rule(
    impl = _hermetic_rust_toolchain_impl,
    attrs = {
        "default_edition": attrs.string(),
        "nix_toolchain_root": attrs.string(),
        "rustc_target_triple": attrs.string(),
    },
    is_toolchain_rule = True,
)
