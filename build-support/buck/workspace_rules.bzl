load("@prelude//rust:cargo_buildscript.bzl", "buildscript_run")

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

def wasm_select(wasm, native):
    """Select a value for wasm32 (wt-web's target) or any native target.

    A wasm32 build carries the features and dependencies Cargo resolves for
    `cargo build -p wt-web --target wasm32-unknown-unknown`, which are not the
    ones the desktop applications ask for.
    """
    return select({
        "config//cpu/constraints:wasm32": wasm,
        "DEFAULT": native,
    })

def _profile_rustc_flags():
    """Profile flags that apply to a final artifact rather than an rlib.

    Cargo's [profile.release] carries lto = "thin". rustc only honours -Clto
    when emitting an executable, so it cannot live in the toolchain's baseline
    rustc_flags; and the toolchain's rustc_binary_flags would additionally
    reach reindeer's build-script binaries, which Cargo never links with LTO.
    Omitting it entirely is what made Buck release binaries 25 to 40 percent
    larger than the Cargo ones they replaced.
    """
    return select({
        "toolchains//:release": ["-Clto=thin"],
        "DEFAULT": [],
    })

def native_binary_alias(name, actual):
    # The build mode comes from the configuration: `?release` on the command
    # line selects release, and no modifier is debug.
    native.alias(
        name = name,
        actual = actual,
        visibility = ["PUBLIC"],
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

def workspace_library(name, crate, package, version, edition = _DEFAULT_EDITION, features = [], deps = [], buildscript = None, resources = [], rustc_flags = [], incoming_transition = None, target_compatible_with = []):
    env = _cargo_env(crate, package, version)
    if buildscript != None:
        env["OUT_DIR"] = "$(location :{}[out_dir])".format(buildscript)
        rustc_flags = ["@$(location :{}[rustc_flags])".format(buildscript)] + rustc_flags

    kwargs = {}
    if incoming_transition != None:
        kwargs["incoming_transition"] = incoming_transition
    if target_compatible_with:
        kwargs["target_compatible_with"] = target_compatible_with

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
        **kwargs
    )

def workspace_test(name, crate, package, version, crate_root, edition = _DEFAULT_EDITION, features = [], deps = [], test_deps = [], buildscript = None, resources = [], run_env = {}, incoming_transition = None):
    """The crate's own #[test] functions, as a Buck test target.

    test_deps are Cargo's [dev-dependencies]. A third-party one reaches
    //third-party/rust through build-support/buck/reindeer-features.
    """
    env = _cargo_env(crate, package, version)
    rustc_flags = []
    if buildscript != None:
        env["OUT_DIR"] = "$(location :{}[out_dir])".format(buildscript)
        rustc_flags = ["@$(location :{}[rustc_flags])".format(buildscript)]

    # Tests read fixtures through env!("CARGO_MANIFEST_DIR"), some of them
    # outside the crate, so it names the crate's source directory as under
    # Cargo rather than Buck's copy of its srcs. rustc_action makes the path
    # absolute.
    manifest = "_cargo_manifest"
    if not native.rule_exists(manifest):
        native.export_file(name = manifest, src = "Cargo.toml", mode = "reference", visibility = [])
    env["CARGO_MANIFEST_DIR"] = "$(location :{})/..".format(manifest)

    kwargs = {}
    if incoming_transition != None:
        kwargs["incoming_transition"] = incoming_transition

    native.rust_test(
        name = name,
        srcs = glob(["**"], exclude = ["BUCK", "Cargo.lock"]) + resources,
        crate = crate,
        crate_root = crate_root,
        edition = edition,
        env = env,
        # Tests run from the project root. insta otherwise runs `cargo
        # metadata` to find the root its snapshot paths are relative to.
        run_env = {
            "CARGO_MANIFEST_DIR": native.package_name(),
            "INSTA_WORKSPACE_ROOT": ".",
        } | run_env,
        features = features,
        rustc_flags = rustc_flags,
        resources = resources,
        visibility = ["PUBLIC"],
        deps = deps + test_deps,
        **kwargs
    )

def workspace_integration_tests(package, version, library, deps = [], test_deps = [], edition = _DEFAULT_EDITION, features = [], exclude = [], run_env = {}):
    """One test target per file directly under tests/, as Cargo builds them.

    Each links `library`, the crate under test, as an integration test does.
    A test that reaches the network is marked #[ignore] in its source, so these
    run offline.
    """
    for path in glob(["tests/*.rs"], exclude = exclude):
        stem = path.removeprefix("tests/").removesuffix(".rs")
        workspace_test(
            name = stem + "_test",
            crate = stem,
            crate_root = path,
            package = package,
            version = version,
            edition = edition,
            features = features,
            deps = [library] + deps,
            test_deps = test_deps,
            run_env = run_env,
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
