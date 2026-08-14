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

def _hermetic_tool(name):
    path = read_root_config("hermetic_tools", name)
    if path == None:
        fail("Missing [hermetic_tools] {}. Run the platform toolchain bootstrap (`nu scripts/refresh-buck-toolchain.nu`, or `toolchains/windows/verify-toolchain.ps1` on Windows) before invoking Buck2.".format(name))
    return path

def native_buildscript_env():
    """Environment forced onto every vendored crate's build script.

    Cargo build scripts otherwise resolve `cc`, `nasm`, and the build profile
    from ambient state. Every one of them is pinned here instead, so a build
    script cannot pick up a compiler that is merely on PATH.
    """
    if _native_build_mode() == "release":
        env = {
            "DEBUG": "false",
            "OPT_LEVEL": "3",
            "PROFILE": "release",
        }
    else:
        env = {
            "DEBUG": "true",
            "OPT_LEVEL": "0",
            "PROFILE": "debug",
        }

    for var, key in [("AR", "ar"), ("CC", "cc"), ("CXX", "cxx"), ("NASM", "nasm")]:
        env[var] = _hermetic_tool(key)

    # MSVC needs its header and library search paths passed explicitly; the Nix
    # toolchains encode theirs in the compiler wrapper.
    for var, key in [("INCLUDE", "include"), ("LIB", "lib")]:
        value = read_root_config("hermetic_tools", key)
        if value != None:
            env[var] = value

    return env

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
    linker_type = LinkerType(ctx.attrs.linker_type)

    # Clang resolves `ld` through PATH, which the actions clear. Point it at the
    # pinned lld in the same Nix toolchain root instead.
    extra_linker_flags = ["-fuse-ld=" + _toolchain_root() + "/bin/ld.lld"] if ctx.attrs.use_lld else []
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
                # Apple's ar predates @argfile support; GNU ar accepts it.
                archiver_supports_argfiles = ctx.attrs.archiver_supports_argfiles,
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
                linker_flags = ["-L" + _toolchain_root() + "/lib"] + extra_linker_flags,
                lto_mode = LtoMode("none"),
                object_file_extension = "o",
                post_linker_flags = [],
                shared_dep_runtime_ld_flags = [],
                shared_library_name_default_prefix = "lib",
                shared_library_name_format = ctx.attrs.shared_library_name_format,
                shared_library_versioned_name_format = ctx.attrs.shared_library_versioned_name_format,
                shlib_interfaces = ShlibInterfacesMode("disabled"),
                static_dep_runtime_ld_flags = [],
                static_library_extension = "a",
                static_pic_dep_runtime_ld_flags = [],
                type = linker_type,
                use_archiver_flags = True,
            ),
            llvm_link = _tool("llvm-link"),
            pic_behavior = PicBehavior(ctx.attrs.pic_behavior),
            use_dep_files = True,
        ),
        CxxPlatformInfo(name = ctx.attrs.platform_name),
    ]

hermetic_cxx_toolchain = rule(
    impl = _hermetic_cxx_toolchain_impl,
    attrs = {
        "archiver_supports_argfiles": attrs.bool(),
        "linker_type": attrs.string(),
        "pic_behavior": attrs.string(),
        "platform_name": attrs.string(),
        "shared_library_name_format": attrs.string(),
        "shared_library_versioned_name_format": attrs.string(),
        "use_lld": attrs.bool(default = False),
        "internal_tools": attrs.default_only(attrs.exec_dep(
            providers = [CxxInternalTools],
            default = "prelude//cxx/tools:internal_tools",
        )),
    },
    is_toolchain_rule = True,
)

def nix_cxx_toolchain(name, os, visibility):
    """Declare the Nix-rooted C++ toolchain for one host operating system."""
    if os == "macos":
        hermetic_cxx_toolchain(
            name = name,
            archiver_supports_argfiles = False,
            linker_type = "darwin",
            pic_behavior = "always_enabled",
            platform_name = "macos-arm64",
            shared_library_name_format = "{}.dylib",
            shared_library_versioned_name_format = "{}.dylib.{}",
            visibility = visibility,
        )
    elif os == "linux":
        hermetic_cxx_toolchain(
            name = name,
            archiver_supports_argfiles = True,
            linker_type = "gnu",
            pic_behavior = "supported",
            platform_name = "linux-x86_64",
            shared_library_name_format = "{}.so",
            shared_library_versioned_name_format = "{}.so.{}",
            use_lld = True,
            visibility = visibility,
        )
    else:
        fail("nix_cxx_toolchain does not support os {}".format(os))

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

def _selected_toolchain_impl(ctx):
    return ctx.attrs.actual.providers

selected_toolchain = rule(
    impl = _selected_toolchain_impl,
    attrs = {"actual": attrs.toolchain_dep()},
    is_toolchain_rule = True,
)
