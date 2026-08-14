load("@prelude//rust:cargo_buildscript.bzl", "buildscript_run")
load("@prelude//rust:cargo_package.bzl", "cargo")
load("@prelude//rust/buildscript:buildscript_platform.bzl", "transition_alias")

def _cargo_env(crate, package, version):
    version_parts = version.split(".")
    return {
        "CARGO_CRATE_NAME": crate,
        "CARGO_MANIFEST_DIR": ".",
        "CARGO_PKG_AUTHORS": "",
        "CARGO_PKG_DESCRIPTION": package,
        "CARGO_PKG_NAME": package,
        "CARGO_PKG_VERSION": version,
        "CARGO_PKG_VERSION_MAJOR": version_parts[0],
        "CARGO_PKG_VERSION_MINOR": version_parts[1],
        "CARGO_PKG_VERSION_PATCH": version_parts[2],
        "CARGO_PKG_VERSION_PRE": "",
    }

def os_select(macos, linux, windows):
    """Select a value by target operating system.

    Mirrors the `[target.'cfg(target_os = ...)']` sections of a Cargo manifest so
    first-party targets can express the same platform-conditional dependencies
    and features that Cargo resolves from cfg predicates.
    """
    return select({
        "config//os/constraints:linux": linux,
        "config//os/constraints:macos": macos,
        "config//os/constraints:windows": windows,
    })

def native_binary_alias(name, actual):
    mode = read_config("native_build", "mode", "debug")
    if mode not in ["debug", "release"]:
        fail("native_build.mode must be debug or release, got {}".format(mode))
    transition_alias(
        name = name,
        actual = actual,
        incoming_transition = "toolchains//:{}_transition".format(mode),
    )

def workspace_buildscript(name, crate, package, version, deps = [], env = {}, srcs = None, rustc_flags = []):
    if srcs == None:
        srcs = glob(["**"], exclude = ["BUCK", "Cargo.lock"])

    build_name = name + "-build"
    run_name = name + "-run"
    cargo.rust_binary(
        name = build_name,
        srcs = srcs,
        crate = "build_script_build",
        crate_root = "build.rs",
        edition = "2024",
        env = _cargo_env("build_script_build", package, version),
        rustc_flags = rustc_flags,
        visibility = [],
        deps = deps,
    )
    buildscript_run(
        name = run_name,
        buildscript_rule = ":" + build_name,
        package_name = package,
        version = version,
        local_manifest_dir = "src",
        env = _cargo_env("build_script_build", package, version) | {
            "WOWS_GAME_DATA": "$(location //build-support/no-game-data:versions_toml)",
            "WOWS_HERMETIC_BUILD": "1",
        } | env,
        visibility = [],
    )
    return run_name

def workspace_library(name, crate, package, version, features = [], deps = [], buildscript = None, resources = []):
    env = _cargo_env(crate, package, version)
    rustc_flags = []
    if buildscript != None:
        env["OUT_DIR"] = "$(location :{}[out_dir])".format(buildscript)
        rustc_flags = ["@$(location :{}[rustc_flags])".format(buildscript)]

    cargo.rust_library(
        name = name,
        srcs = glob(["**"], exclude = ["BUCK", "Cargo.lock"]) + resources,
        crate = crate,
        crate_root = "src/lib.rs",
        edition = "2024",
        env = env,
        features = features,
        rustc_flags = rustc_flags,
        resources = resources,
        visibility = ["PUBLIC"],
        deps = deps,
    )

def workspace_binary(name, crate, package, version, crate_root, features = [], deps = [], buildscript = None, resources = [], rustc_flags = []):
    env = _cargo_env(crate, package, version)
    if buildscript != None:
        env["OUT_DIR"] = "$(location :{}[out_dir])".format(buildscript)
        rustc_flags = ["@$(location :{}[rustc_flags])".format(buildscript)] + rustc_flags

    cargo.rust_binary(
        name = name,
        srcs = glob(["**"], exclude = ["BUCK", "Cargo.lock"]),
        crate = crate,
        crate_root = crate_root,
        edition = "2024",
        env = env,
        features = features,
        rustc_flags = rustc_flags,
        resources = resources,
        visibility = ["PUBLIC"],
        deps = deps,
    )
