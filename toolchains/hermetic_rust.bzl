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
load("@prelude//tests:remote_test_execution_toolchain.bzl", "RemoteTestExecutionToolchainInfo")

# The Nix toolchain roots Buck reads from .buckconfig.local. `nix_toolchain`
# serves every host target; `nix_windows_cross` (flake output
# buck-windows-cross, Linux only) builds the Windows executables the Linux
# launcher runs under Wine. Read at load time: analysis cannot read config.
_TOOLCHAIN_ROOTS = {
    "nix_toolchain": read_root_config("nix_toolchain", "root"),
    "nix_windows_cross": read_root_config("nix_windows_cross", "root"),
}

def _toolchain_root(section = "nix_toolchain"):
    root = _TOOLCHAIN_ROOTS[section]
    if root == None:
        fail("Missing [{}] root. Run `nu scripts/refresh-buck-toolchain.nu` before invoking Buck2.".format(section))
    return root

def _tool(name, section = "nix_toolchain"):
    return RunInfo(args = [_toolchain_root(section) + "/bin/" + name])

def windows_cross_compatible_with():
    """Constraints for a target that builds only with the Windows cross toolchain.

    Hosts without a `[nix_windows_cross]` root (Windows, macOS) skip it, so
    patterns such as `root//...` do not fail analysis.
    """
    return [] if _TOOLCHAIN_ROOTS["nix_windows_cross"] else ["prelude//:none"]

def _windows_cross_resource_impl(ctx):
    root = _toolchain_root("nix_windows_cross")
    output = ctx.actions.declare_output(ctx.attrs.out)

    # windres preprocesses through the cross gcc, which it would otherwise
    # seek on PATH.
    ctx.actions.run(
        cmd_args(
            root + "/bin/x86_64-w64-mingw32-windres",
            "--preprocessor=" + root + "/bin/x86_64-w64-mingw32-gcc",
            "--preprocessor-arg=-E",
            "--preprocessor-arg=-xc-header",
            "--preprocessor-arg=-DRC_INVOKED",
            "-O",
            "coff",
            "-i",
            ctx.attrs.src,
            "-o",
            output.as_output(),
        ),
        category = "windres",
    )
    return [DefaultInfo(default_output = output)]

# A resource script compiled to a COFF object, linked into a windows-gnu
# executable with `-Clink-arg=$(location ...)`.
windows_cross_resource = rule(
    impl = _windows_cross_resource_impl,
    attrs = {
        "out": attrs.string(),
        "src": attrs.source(),
    },
)

# The build mode is the `toolchains//:native_build_mode` constraint, which the
# `release` modifier sets (`buck2 build //:wows_toolkit_egui?release`). Debug is the
# configuration without it.
_RELEASE = "toolchains//:release"

# How much debug info a debug build carries. Release never carries any. CI reads
# nothing out of the artifacts it builds, and full debug info is what makes
# linking a binary the size of the GUI expensive, so the bootstrap sets this to
# `line-tables-only` there: a panic in a Buck test still names a file and line,
# without the rest of the DWARF the link would have to copy.
_DEBUGINFO_LEVELS = {
    "full": "2",
    "line-tables-only": "1",
    "none": "0",
}

def _debug_debuginfo():
    asked = read_root_config("native_build", "debuginfo", "full")
    level = _DEBUGINFO_LEVELS.get(asked)
    if level == None:
        fail("[native_build] debuginfo is {}; it must be one of {}.".format(
            asked,
            ", ".join(sorted(_DEBUGINFO_LEVELS)),
        ))
    return level

def _rustc_flags():
    return select({
        _RELEASE: ["-Copt-level=3", "-Cdebuginfo=0"],
        "DEFAULT": ["-Copt-level=0", "-Cdebuginfo=" + _debug_debuginfo()],
    })

def _cxx_compiler_flags():
    return select({
        _RELEASE: ["-O3"],
        "DEFAULT": ["-O0", "-g"],
    })

def _hermetic_tool(name):
    path = read_root_config("hermetic_tools", name)
    if path == None:
        fail("Missing [hermetic_tools] {}. Run the platform toolchain bootstrap (`nu scripts/refresh-buck-toolchain.nu`, or `toolchains/windows/verify-toolchain.ps1` on Windows) before invoking Buck2.".format(name))
    return path

def native_buildscript_env():
    """Environment forced onto every vendored crate's build script.

    Cargo build scripts otherwise resolve `cc`, `nasm`, `python3`, and the
    build profile from ambient state. Every one of them is pinned here instead,
    so a build script cannot pick up a tool that is merely on PATH.
    """
    env = {
        "DEBUG": select({_RELEASE: "false", "DEFAULT": "true"}),
        "OPT_LEVEL": select({_RELEASE: "3", "DEFAULT": "0"}),
        "PROFILE": select({_RELEASE: "release", "DEFAULT": "debug"}),
    }

    for var, key in [("AR", "ar"), ("CC", "cc"), ("CXX", "cxx"), ("NASM", "nasm"), ("PYTHON3", "python")]:
        env[var] = _hermetic_tool(key)

    # gpui's Windows backend compiles its HLSL with fxc, which it otherwise
    # locates through the Windows SDK's registry entry. Absent on the Unix
    # toolchains, where nothing in the graph compiles a shader.
    fxc = read_root_config("hermetic_tools", "fxc")
    if fxc != None:
        env["GPUI_FXC_PATH"] = fxc

    # MSVC needs its header and library search paths passed explicitly; the Nix
    # toolchains encode theirs in the compiler wrapper.
    for var, key in [("INCLUDE", "include"), ("LIB", "lib")]:
        value = read_root_config("hermetic_tools", key)
        if value != None:
            env[var] = value

    return env

def _hermetic_rust_toolchain_impl(ctx):
    section = ctx.attrs.toolchain_root_section

    # windows-gnu imports raw-dylib functions (windows-sys) through an import
    # library rustc writes with dlltool, which it would otherwise seek on PATH.
    dlltool_flags = ["-Cdlltool=" + _toolchain_root(section) + "/bin/" + ctx.attrs.dlltool] if ctx.attrs.dlltool else []
    return [
        DefaultInfo(),
        RustToolchainInfo(
            allow_lints = [],
            clippy_driver = _tool("clippy-driver", section),
            clippy_toml = None,
            compiler = _tool("rustc", section),
            default_edition = ctx.attrs.default_edition,
            deny_lints = [],
            doctests = False,
            nightly_features = False,
            panic_runtime = PanicRuntime("unwind"),
            report_unused_deps = False,
            rustc_binary_flags = [],
            rustc_flags = ctx.attrs.rustc_flags + dlltool_flags,
            rustc_target_triple = ctx.attrs.rustc_target_triple,
            rustc_test_flags = [],
            rustdoc = _tool("rustdoc", section),
            rustdoc_flags = [],
            warn_lints = [],
        ),
    ]

_hermetic_rust_toolchain_rule = rule(
    impl = _hermetic_rust_toolchain_impl,
    attrs = {
        "default_edition": attrs.string(),
        "dlltool": attrs.option(attrs.string(), default = None),
        "rustc_flags": attrs.list(attrs.string()),
        "rustc_target_triple": attrs.string(),
        "toolchain_root_section": attrs.string(default = "nix_toolchain"),
    },
    is_toolchain_rule = True,
)

def hermetic_rust_toolchain(**kwargs):
    """The Nix-rooted Rust toolchain, compiling in the selected build mode."""
    _hermetic_rust_toolchain_rule(rustc_flags = _rustc_flags(), **kwargs)

def _hermetic_rustfmt_impl(ctx):
    if not ctx.attrs.rustfmt:
        fail("Missing [hermetic_tools] rustfmt. Run the platform toolchain bootstrap (`nu scripts/refresh-buck-toolchain.nu`, or `toolchains/windows/verify-toolchain.ps1` on Windows) before linting.")
    return [DefaultInfo(), RunInfo(args = [ctx.attrs.rustfmt])]

_hermetic_rustfmt_rule = rule(
    impl = _hermetic_rustfmt_impl,
    attrs = {"rustfmt": attrs.string()},
)

def hermetic_rustfmt(name, visibility):
    """A plain runnable rustfmt.

    RustToolchainInfo has no rustfmt field, so bxl/lint.bxl resolves this target
    and scripts/lint.nu runs what it points at, rather than whatever rustfmt is
    on PATH. Both platform bootstraps publish the path, so one rule covers all
    three; read_root_config is load-time only, hence the macro.

    Resolved with a default rather than a load-time fail: this package is loaded
    by every buck2 command, and only linting needs a rustfmt.
    """
    _hermetic_rustfmt_rule(
        name = name,
        rustfmt = read_root_config("hermetic_tools", "rustfmt", ""),
        visibility = visibility,
    )

WasmToolsInfo = provider(fields = {
    "wasm_bindgen": provider_field(RunInfo),
    "wasm_opt": provider_field(RunInfo),
})

def _nix_wasm_tools_impl(_ctx):
    return [
        DefaultInfo(),
        WasmToolsInfo(
            wasm_bindgen = _tool("wasm-bindgen"),
            wasm_opt = _tool("wasm-opt"),
        ),
    ]

# The post-link tools of a wasm site (build-support/buck/wasm_rules.bzl).
nix_wasm_tools = rule(
    impl = _nix_wasm_tools_impl,
    attrs = {},
    is_toolchain_rule = True,
)

def _local_remote_test_execution_impl(_ctx):
    # Every action in this repo runs locally, and CI additionally runs Buck in a
    # network namespace, so there is no remote profile to offer.
    return [
        DefaultInfo(),
        RemoteTestExecutionToolchainInfo(
            default_profile = None,
            profiles = {},
            default_run_as_bundle = False,
        ),
    ]

local_remote_test_execution_toolchain = rule(
    impl = _local_remote_test_execution_impl,
    attrs = {},
    is_toolchain_rule = True,
)

def _hermetic_cxx_toolchain_impl(ctx):
    linker_type = LinkerType(ctx.attrs.linker_type)
    section = ctx.attrs.toolchain_root_section
    root = _toolchain_root(section)
    prefix = ctx.attrs.tool_prefix
    compiler_type = ctx.attrs.compiler_type

    def tool(name):
        return _tool(name, section)

    # Clang resolves `ld` through PATH, which the actions clear. Point it at the
    # pinned lld in the same Nix toolchain root instead.
    extra_linker_flags = ["-fuse-ld=" + root + "/bin/ld.lld"] if ctx.attrs.use_lld else []
    linker_flags = (["-L" + root + "/lib"] if ctx.attrs.link_toolchain_lib else []) + extra_linker_flags
    return [
        DefaultInfo(),
        CxxToolchainInfo(
            as_compiler_info = CCompilerInfo(
                compiler = tool(ctx.attrs.c_compiler),
                compiler_type = compiler_type,
            ),
            asm_compiler_info = CCompilerInfo(
                compiler = tool(ctx.attrs.c_compiler),
                compiler_type = compiler_type,
            ),
            binary_utilities_info = BinaryUtilitiesInfo(
                nm = tool(prefix + "nm"),
                objcopy = tool(prefix + "objcopy"),
                objdump = tool(prefix + "objdump"),
                ranlib = tool(prefix + "ranlib"),
                strip = tool(prefix + "strip"),
            ),
            bolt_enabled = False,
            c_compiler_info = CCompilerInfo(
                compiler = tool(ctx.attrs.c_compiler),
                compiler_type = compiler_type,
                compiler_flags = ctx.attrs.compiler_flags,
                preprocessor_flags = [],
                supports_content_based_paths = False,
                supports_two_phase_compilation = False,
            ),
            cpp_dep_tracking_mode = DepTrackingMode("show_headers"),
            cxx_compiler_info = CxxCompilerInfo(
                compiler = tool(ctx.attrs.cxx_compiler),
                compiler_type = compiler_type,
                compiler_flags = ctx.attrs.compiler_flags,
                preprocessor_flags = [],
                supports_content_based_paths = False,
                supports_two_phase_compilation = False,
            ),
            header_mode = HeaderMode("symlink_tree_only"),
            internal_tools = ctx.attrs.internal_tools[CxxInternalTools],
            linker_info = LinkerInfo(
                archiver = tool(prefix + ctx.attrs.archiver),
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
                linker = tool(ctx.attrs.linker),
                linker_flags = linker_flags,
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
            llvm_link = tool("llvm-link"),
            pic_behavior = PicBehavior(ctx.attrs.pic_behavior),
            runtime_dependency_handling = RuntimeDependencyHandling("no_symlink"),
            use_dep_files = True,
        ),
        CxxPlatformInfo(name = ctx.attrs.platform_name),
    ]

hermetic_cxx_toolchain = rule(
    impl = _hermetic_cxx_toolchain_impl,
    attrs = {
        "archiver": attrs.string(default = "ar"),
        "archiver_supports_argfiles": attrs.bool(),
        "c_compiler": attrs.string(default = "clang"),
        "compiler_type": attrs.string(default = "clang"),
        "compiler_flags": attrs.list(attrs.string()),
        "cxx_compiler": attrs.string(default = "clang++"),
        # The Nix wrappers link through clang, which needs the toolchain's own
        # libraries on the search path.
        "link_toolchain_lib": attrs.bool(default = True),
        "linker": attrs.string(default = "clang++"),
        "linker_type": attrs.string(),
        "pic_behavior": attrs.string(),
        "platform_name": attrs.string(),
        "shared_library_name_format": attrs.string(),
        "shared_library_versioned_name_format": attrs.string(),
        # Prepended to the binutils names, for a cross toolchain whose tools
        # carry the target triple.
        "tool_prefix": attrs.string(default = ""),
        "toolchain_root_section": attrs.string(default = "nix_toolchain"),
        "use_lld": attrs.bool(default = False),
        "internal_tools": attrs.default_only(attrs.exec_dep(
            providers = [CxxInternalTools],
            default = "prelude//cxx/tools:internal_tools",
        )),
    },
    is_toolchain_rule = True,
)

def nix_cxx_toolchain(name, os, visibility):
    """Declare the Nix-rooted C++ toolchain for one host operating system, for
    wasm32 (`os = "none"`), or for Windows cross builds (`os = "windows-gnu"`)."""
    if os == "macos":
        hermetic_cxx_toolchain(
            name = name,
            archiver_supports_argfiles = False,
            compiler_flags = _cxx_compiler_flags(),
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
            compiler_flags = _cxx_compiler_flags(),
            linker_type = "gnu",
            pic_behavior = "supported",
            platform_name = "linux-x86_64",
            shared_library_name_format = "{}.so",
            shared_library_versioned_name_format = "{}.so.{}",
            use_lld = True,
            visibility = visibility,
        )
    elif os == "none":
        hermetic_cxx_toolchain(
            name = name,
            archiver = "llvm-ar",
            archiver_supports_argfiles = True,
            c_compiler = "wasm32-unknown-unknown-clang",
            compiler_flags = ["--target=wasm32-unknown-unknown"] + _cxx_compiler_flags(),
            cxx_compiler = "wasm32-unknown-unknown-clang",
            # rustc calls the linker as wasm-ld and puts `-flavor wasm` first,
            # which lld reads only as the first argument, so nothing may
            # precede it.
            link_toolchain_lib = False,
            linker = "wasm-ld",
            linker_type = "wasm",
            pic_behavior = "not_supported",
            platform_name = "wasm32",
            shared_library_name_format = "{}.wasm",
            shared_library_versioned_name_format = "{}.wasm",
            visibility = visibility,
        )
    elif os == "windows-gnu":
        hermetic_cxx_toolchain(
            name = name,
            archiver_supports_argfiles = True,
            c_compiler = "x86_64-w64-mingw32-gcc",
            compiler_flags = _cxx_compiler_flags(),
            compiler_type = "gcc",
            cxx_compiler = "x86_64-w64-mingw32-g++",
            # The windows-gnu std links winpthreads statically from here.
            link_toolchain_lib = True,
            linker = "x86_64-w64-mingw32-gcc",
            linker_type = "gnu",
            pic_behavior = "not_supported",
            platform_name = "windows-x86_64-gnu",
            shared_library_name_format = "{}.dll",
            shared_library_versioned_name_format = "{}.dll",
            tool_prefix = "x86_64-w64-mingw32-",
            toolchain_root_section = "nix_windows_cross",
            visibility = visibility,
        )
    else:
        fail("nix_cxx_toolchain does not support os {}".format(os))

def _hermetic_python_bootstrap_toolchain_impl(ctx):
    return [
        DefaultInfo(),
        PythonBootstrapToolchainInfo(interpreter = ctx.attrs.interpreter),
    ]

_hermetic_python_bootstrap_toolchain_rule = rule(
    impl = _hermetic_python_bootstrap_toolchain_impl,
    attrs = {"interpreter": attrs.string()},
    is_toolchain_rule = True,
)

def hermetic_python_bootstrap_toolchain(name, visibility):
    """The prelude's C++ internal tools are Python scripts, so every platform
    needs an interpreter, not just the Nix-rooted ones."""

    # Resolved while the package loads; read_root_config is unavailable during
    # analysis on some rule kinds.
    _hermetic_python_bootstrap_toolchain_rule(
        name = name,
        interpreter = _hermetic_tool("python"),
        visibility = visibility,
    )

def _selected_toolchain_impl(ctx):
    return ctx.attrs.actual.providers

_selected_toolchain_rule = rule(
    impl = _selected_toolchain_impl,
    attrs = {"actual": attrs.toolchain_dep()},
    is_toolchain_rule = True,
)

def selected_toolchain(name, actual, visibility):
    # Build scripts and proc macros then build in the target's mode; see
    # root//platforms:defs.bzl.
    _selected_toolchain_rule(
        name = name,
        actual = actual,
        exec_compatible_with = select({
            _RELEASE: [_RELEASE],
            "DEFAULT": ["toolchains//:debug"],
        }),
        visibility = visibility,
    )

def _missing_system_library_impl(ctx):
    fail("Missing [nix_system_libs] root, which {} needs to link lib{}. Run `nu scripts/refresh-buck-toolchain.nu` before invoking Buck2.".format(ctx.label.raw_target(), ctx.attrs.lib))

_missing_system_library = rule(
    impl = _missing_system_library_impl,
    attrs = {"lib": attrs.string()},
)

# A shared library from the Nix system libraries (flake output
# buck-system-libs), linked dynamically. The target system provides it at run
# time. Only Linux links one.
def nix_system_library(name, lib, visibility):
    root = read_root_config("nix_system_libs", "root")
    if root:
        native.prebuilt_cxx_library(
            name = name,
            header_only = True,
            exported_linker_flags = ["-L" + root + "/lib", "-l" + lib],
            target_compatible_with = ["prelude//os:linux"],
            visibility = visibility,
        )
    else:
        # Fails only the builds that link the library, so a host that never
        # links one needs no section.
        _missing_system_library(
            name = name,
            lib = lib,
            target_compatible_with = ["prelude//os:linux"],
            visibility = visibility,
        )
