load(
    "@prelude//cxx:cxx_toolchain_types.bzl",
    "BinaryUtilitiesInfo",
    "CCompilerInfo",
    "CxxCompilerInfo",
    "CxxInternalTools",
    "CxxPlatformInfo",
    "CxxToolchainInfo",
    "DepTrackingMode",
    "LinkerInfo",
    "LinkerType",
    "PicBehavior",
    "RuntimeDependencyHandling",
    "ShlibInterfacesMode",
)
load("@prelude//cxx:headers.bzl", "HeaderMode")
load("@prelude//linking:link_info.bzl", "LinkStyle")
load("@prelude//linking:lto.bzl", "LtoMode")
load("@prelude//python_bootstrap:python_bootstrap.bzl", "PythonBootstrapToolchainInfo")
load("@prelude//rust:rust_toolchain.bzl", "PanicRuntime", "RustToolchainInfo")

def _toolchain_root():
    root = read_root_config("nix_toolchain", "root")
    if root == None:
        fail("Missing [nix_toolchain] root. Run `nu scripts/refresh-buck-toolchain.nu` before invoking Buck2.")
    return root

def _tool(name):
    return RunInfo(args = [_toolchain_root() + "/bin/" + name])

def _native_build_mode():
    mode = read_root_config("native_build", "mode", "debug")
    if mode not in ["debug", "release"]:
        fail("native_build.mode must be debug or release, got {}".format(mode))
    return mode

def _rustc_flags():
    if _native_build_mode() == "release":
        return ["-Copt-level=3", "-Cdebuginfo=0"]
    return ["-Copt-level=0", "-Cdebuginfo=2"]

def _cxx_compiler_flags():
    if _native_build_mode() == "release":
        return ["-O3"]
    return ["-O0", "-g"]

def native_buildscript_env():
    if _native_build_mode() == "release":
        return {
            "DEBUG": "false",
            "OPT_LEVEL": "3",
            "PROFILE": "release",
        }
    return {
        "DEBUG": "true",
        "OPT_LEVEL": "0",
        "PROFILE": "debug",
    }

def validate_nix_toolchain():
    _toolchain_root()

def _hermetic_rust_toolchain_impl(ctx):
    return [
        DefaultInfo(),
        RustToolchainInfo(
            allow_lints = [],
            clippy_driver = _tool("clippy-driver"),
            clippy_toml = None,
            compiler = _tool("rustc"),
            default_edition = ctx.attrs.default_edition,
            deny_lints = [],
            doctests = False,
            nightly_features = False,
            panic_runtime = PanicRuntime("unwind"),
            report_unused_deps = False,
            rustc_binary_flags = [],
            rustc_flags = _rustc_flags(),
            rustc_target_triple = ctx.attrs.rustc_target_triple,
            rustc_test_flags = [],
            rustdoc = _tool("rustdoc"),
            rustdoc_flags = [],
            warn_lints = [],
        ),
    ]

hermetic_rust_toolchain = rule(
    impl = _hermetic_rust_toolchain_impl,
    attrs = {
        "default_edition": attrs.string(),
        "rustc_target_triple": attrs.string(),
    },
    is_toolchain_rule = True,
)

def _hermetic_cxx_toolchain_impl(ctx):
    return [
        DefaultInfo(),
        CxxToolchainInfo(
            as_compiler_info = CCompilerInfo(
                compiler = _tool("clang"),
                compiler_type = "clang",
            ),
            asm_compiler_info = CCompilerInfo(
                compiler = _tool("clang"),
                compiler_type = "clang",
            ),
            binary_utilities_info = BinaryUtilitiesInfo(
                nm = _tool("nm"),
                objcopy = _tool("objcopy"),
                objdump = _tool("objdump"),
                ranlib = _tool("ranlib"),
                strip = _tool("strip"),
            ),
            bolt_enabled = False,
            c_compiler_info = CCompilerInfo(
                compiler = _tool("clang"),
                compiler_type = "clang",
                compiler_flags = _cxx_compiler_flags(),
                preprocessor_flags = [],
                supports_content_based_paths = False,
                supports_two_phase_compilation = False,
            ),
            cpp_dep_tracking_mode = DepTrackingMode("show_headers"),
            cxx_compiler_info = CxxCompilerInfo(
                compiler = _tool("clang++"),
                compiler_type = "clang",
                compiler_flags = _cxx_compiler_flags(),
                preprocessor_flags = [],
                supports_content_based_paths = False,
                supports_two_phase_compilation = False,
            ),
            header_mode = HeaderMode("symlink_tree_only"),
            internal_tools = ctx.attrs.internal_tools[CxxInternalTools],
            linker_info = LinkerInfo(
                archiver = _tool("ar"),
                archiver_supports_argfiles = False,
                archiver_type = "gnu",
                archive_objects_locally = True,
                binary_extension = "",
                force_full_hybrid_if_capable = False,
                generate_linker_maps = False,
                independent_shlib_interface_linker_flags = [],
                is_pdb_generated = False,
                link_binaries_locally = True,
                link_libraries_locally = True,
                link_style = LinkStyle("shared"),
                link_weight = 1,
                linker = _tool("clang++"),
                linker_flags = ["-L" + _toolchain_root() + "/lib"],
                lto_mode = LtoMode("none"),
                object_file_extension = "o",
                post_linker_flags = [],
                shared_dep_runtime_ld_flags = [],
                shared_library_name_default_prefix = "lib",
                shared_library_name_format = "{}.dylib",
                shared_library_versioned_name_format = "{}.dylib.{}",
                shlib_interfaces = ShlibInterfacesMode("disabled"),
                static_dep_runtime_ld_flags = [],
                static_library_extension = "a",
                static_pic_dep_runtime_ld_flags = [],
                type = LinkerType("darwin"),
                use_archiver_flags = True,
            ),
            llvm_link = _tool("llvm-link"),
            pic_behavior = PicBehavior("always_enabled"),
            runtime_dependency_handling = RuntimeDependencyHandling("no_symlink"),
            use_dep_files = True,
        ),
        CxxPlatformInfo(name = "macos-arm64"),
    ]

hermetic_cxx_toolchain = rule(
    impl = _hermetic_cxx_toolchain_impl,
    attrs = {
        "internal_tools": attrs.default_only(attrs.exec_dep(
            providers = [CxxInternalTools],
            default = "prelude//cxx/tools:internal_tools",
        )),
    },
    is_toolchain_rule = True,
)

def _hermetic_python_bootstrap_toolchain_impl(_ctx):
    return [
        DefaultInfo(),
        PythonBootstrapToolchainInfo(interpreter = _toolchain_root() + "/bin/python3"),
    ]

hermetic_python_bootstrap_toolchain = rule(
    impl = _hermetic_python_bootstrap_toolchain_impl,
    attrs = {},
    is_toolchain_rule = True,
)
