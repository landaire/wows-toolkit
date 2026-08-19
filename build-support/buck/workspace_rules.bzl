load("@prelude//rust:cargo_buildscript.bzl", "buildscript_run")
load("@prelude//rust/buildscript:buildscript_platform.bzl", "transition_alias")

# Every crate but wgcheck is on the workspace edition; a crate that pins its own
# has to say so here too, because the Rust rules cannot read Cargo.toml.
# scripts/test-workspace-package-metadata.nu fails when the two disagree.
_DEFAULT_EDITION = "2024"

def _cargo_env(crate, package, version):
    # A pre-release suffix belongs to _PRE, not to _PATCH, which is what a
    # plain split on "." would produce for 1.0.2-beta1.
    core, _, pre = version.partition("-")
    version_parts = core.split(".")
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
        "CARGO_PKG_VERSION_PRE": pre,
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

def _native_build_mode():
    mode = read_config("native_build", "mode", "debug")
    if mode not in ["debug", "release"]:
        fail("native_build.mode must be debug or release, got {}".format(mode))
    return mode

def _profile_rustc_flags():
    """Profile flags that apply to a final artifact rather than an rlib.

    Cargo's [profile.release] carries lto = "thin". rustc only honours -Clto
    when emitting an executable, so it cannot live in the toolchain's baseline
    rustc_flags; and the toolchain's rustc_binary_flags would additionally
    reach reindeer's build-script binaries, which Cargo never links with LTO.
    Omitting it entirely is what made Buck release binaries 25 to 40 percent
    larger than the Cargo ones they replaced.
    """
    if _native_build_mode() == "release":
        return ["-Clto=thin"]
    return []

def native_binary_alias(name, actual):
    mode = _native_build_mode()
    transition_alias(
        name = name,
        actual = actual,
        incoming_transition = "toolchains//:{}_transition".format(mode),
    )

def workspace_buildscript(name, crate, package, version, edition = _DEFAULT_EDITION, deps = [], env = {}, srcs = None, rustc_flags = []):
    if srcs == None:
        srcs = glob(["**"], exclude = ["BUCK", "Cargo.lock"])

    build_name = name + "-build"
    run_name = name + "-run"
    # native, not cargo.rust_binary: build.rs is first-party code, and the cargo
    # wrapper would cap its lints to allow like a vendored crate's.
    native.rust_binary(
        name = build_name,
        srcs = srcs,
        crate = "build_script_build",
        crate_root = "build.rs",
        edition = edition,
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

def workspace_library(name, crate, package, version, edition = _DEFAULT_EDITION, features = [], deps = [], buildscript = None, resources = []):
    env = _cargo_env(crate, package, version)
    rustc_flags = []
    if buildscript != None:
        env["OUT_DIR"] = "$(location :{}[out_dir])".format(buildscript)
        rustc_flags = ["@$(location :{}[rustc_flags])".format(buildscript)]

    # native.rust_library, not cargo.rust_library: the cargo wrappers exist for
    # reindeer's vendored crates and prepend --cap-lints=allow, which silently
    # caps every first-party lint too and makes [clippy.json] always empty.
    native.rust_library(
        name = name,
        srcs = glob(["**"], exclude = ["BUCK", "Cargo.lock"]) + resources,
        doctests = False,
        crate = crate,
        crate_root = "src/lib.rs",
        edition = edition,
        env = env,
        features = features,
        rustc_flags = rustc_flags,
        resources = resources,
        visibility = ["PUBLIC"],
        deps = deps,
    )

def workspace_test(name, crate, package, version, crate_root, edition = _DEFAULT_EDITION, features = [], deps = [], test_deps = [], buildscript = None, resources = []):
    """The crate's own #[test] functions, as a Buck test target.

    test_deps are Cargo's [dev-dependencies]. reindeer is configured to vendor
    normal dependencies only, so a dev-dependency is available here only when
    something else in the graph already pulls it in.
    """
    env = _cargo_env(crate, package, version)
    rustc_flags = []
    if buildscript != None:
        env["OUT_DIR"] = "$(location :{}[out_dir])".format(buildscript)
        rustc_flags = ["@$(location :{}[rustc_flags])".format(buildscript)]

    native.rust_test(
        name = name,
        srcs = glob(["**"], exclude = ["BUCK", "Cargo.lock"]) + resources,
        crate = crate,
        crate_root = crate_root,
        edition = edition,
        env = env,
        features = features,
        rustc_flags = rustc_flags,
        resources = resources,
        visibility = ["PUBLIC"],
        deps = deps + test_deps,
    )

def workspace_binary(name, crate, package, version, crate_root, edition = _DEFAULT_EDITION, features = [], deps = [], buildscript = None, resources = [], rustc_flags = []):
    env = _cargo_env(crate, package, version)
    if buildscript != None:
        env["OUT_DIR"] = "$(location :{}[out_dir])".format(buildscript)
        rustc_flags = ["@$(location :{}[rustc_flags])".format(buildscript)] + rustc_flags
    rustc_flags = rustc_flags + _profile_rustc_flags()

    # See workspace_library: the cargo wrapper would cap lints to allow.
    native.rust_binary(
        name = name,
        srcs = glob(["**"], exclude = ["BUCK", "Cargo.lock"]),
        crate = crate,
        crate_root = crate_root,
        edition = edition,
        env = env,
        features = features,
        rustc_flags = rustc_flags,
        resources = resources,
        visibility = ["PUBLIC"],
        deps = deps,
    )
