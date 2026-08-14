load("@prelude//rust:cargo_buildscript.bzl", _buildscript_run = "buildscript_run")
load("@toolchains//:hermetic_rust.bzl", "native_buildscript_env")

def buildscript_run(env = {}, **kwargs):
    """Run a vendored crate's build script under the pinned toolchain.

    The pinned toolchain is authoritative for the build profile and for every
    compiler a build script may invoke, so fixup-declared values are a fallback
    only and no crate can compile against a toolchain that is merely on PATH.
    """
    _buildscript_run(env = env | native_buildscript_env(), **kwargs)
