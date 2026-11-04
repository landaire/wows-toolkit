load("@prelude//rust:cargo_buildscript.bzl", _buildscript_run = "buildscript_run")
load("@prelude//rust:cargo_package.bzl", _cargo = "cargo")
load("@toolchains//:hermetic_rust.bzl", "native_buildscript_env")

def buildscript_run(env = {}, **kwargs):
    """Run a vendored crate's build script under the pinned toolchain.

    The pinned toolchain is authoritative for the build profile and for every
    compiler a build script may invoke, so fixup-declared values are a fallback
    only and no crate can compile against a toolchain that is merely on PATH.
    """
    _buildscript_run(env = env | native_buildscript_env(), **kwargs)

def _rust_library(name, rustc_flags = [], **kwargs):
    # Cargo.toml's [profile.dev.package."*"] builds every dependency at
    # opt-level 2 in debug. A later -C flag overrides the toolchain's. Proc
    # macros run in the compiler, which Cargo builds under build-override.
    if not kwargs.get("proc_macro", False):
        rustc_flags = rustc_flags + select({
            "DEFAULT": ["-Copt-level=2"],
            "toolchains//:release": [],
        })
    _cargo.rust_library(name = name, rustc_flags = rustc_flags, **kwargs)

# Build scripts are left alone: Cargo builds them under build-override too.
cargo = struct(
    rust_binary = _cargo.rust_binary,
    rust_library = _rust_library,
)
