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

# The Nix toolchain root Buck reads from .buckconfig.local. Read at load time:
# analysis cannot read config.
_NIX_TOOLCHAIN_ROOT = read_root_config("nix_toolchain", "root")

def _toolchain_root():
    if _NIX_TOOLCHAIN_ROOT == None:
        fail("Missing [nix_toolchain] root. Run `nu scripts/refresh-buck-toolchain.nu` before invoking Buck2.")
    return _NIX_TOOLCHAIN_ROOT

def _tool(name):
    return RunInfo(args = [_toolchain_root() + "/bin/" + name])

# The Windows cross toolchain runs the Nix toolchain's rustc and lld, and only
# the Linux app ships the executables it builds.
WINDOWS_CROSS_EXEC_COMPATIBLE_WITH = [
    "config//os/constraints:linux",
    "config//cpu/constraints:x86_64",
]

def windows_cross_compatible_with():
    """Constraints for a target that builds only with the Windows cross toolchain.

    Its targets are configured for windows-gnu whatever the host, so the host
    is checked here. Other hosts skip them, so patterns such as `root//...` do
    not fail analysis.
    """
    host = host_info()
    return [] if host.os.is_linux and host.arch.is_x86_64 else ["prelude//:none"]

# The Wine helpers ship beside the Linux launcher and are copied into every
# prefix, so they are built for size. These follow the toolchain's own flags
# and the release `-Clto=thin`, and override them.
WINDOWS_HELPER_RUSTC_FLAGS = [
    "-Copt-level=z",
    "-Ccodegen-units=1",
    "-Clto=fat",
    "-Cpanic=abort",
    "-Cstrip=symbols",
]

def _windows_cross_resource_impl(ctx):
    output = ctx.actions.declare_output(ctx.attrs.out)

    # The scripts use no preprocessor directives. Preprocessing would run the
    # Nix clang, whose wrapper targets the host.
    ctx.actions.run(
        cmd_args(
            _toolchain_root() + "/bin/llvm-windres",
            "--target=pe-x86-64",
            "--no-preprocess",
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

    # MSVC needs its header and library search paths passed explicitly; the Nix
    # toolchains encode theirs in the compiler wrapper.
    for var, key in [("INCLUDE", "include"), ("LIB", "lib")]:
        value = read_root_config("hermetic_tools", key)
        if value != None:
            env[var] = value

    return env

def _hermetic_rust_toolchain_impl(ctx):
    # windows-gnu imports raw-dylib functions (windows-sys) through an import
    # library rustc writes with dlltool, which it would otherwise seek on PATH.
    dlltool_flags = ["-Cdlltool=" + _toolchain_root() + "/bin/" + ctx.attrs.dlltool] if ctx.attrs.dlltool else []
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
            rustc_flags = ctx.attrs.rustc_flags + dlltool_flags,
            rustc_target_triple = ctx.attrs.rustc_target_triple,
            rustc_test_flags = [],
            rustdoc = _tool("rustdoc"),
            rustdoc_flags = [],
            sysroot_path = ctx.attrs.sysroot,
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
        # A sysroot holding the target's standard library, for a target the
        # Nix rustc ships none for.
        "sysroot": attrs.option(attrs.source(allow_directory = True), default = None),
    },
    is_toolchain_rule = True,
)

def hermetic_rust_toolchain(rustc_flags = [], **kwargs):
    """The Nix-rooted Rust toolchain, compiling in the selected build mode."""
    _hermetic_rust_toolchain_rule(rustc_flags = _rustc_flags() + rustc_flags, **kwargs)

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
    root = _toolchain_root()
    prefix = ctx.attrs.tool_prefix
    compiler_type = ctx.attrs.compiler_type

    # Clang resolves `ld` through PATH, which the actions clear. Point it at the
    # pinned lld in the same Nix toolchain root instead.
    extra_linker_flags = ["-fuse-ld=" + root + "/bin/ld.lld"] if ctx.attrs.use_lld else []
    linker_flags = (
        (["-L" + root + "/lib"] if ctx.attrs.link_toolchain_lib else []) +
        [cmd_args("-L", directory, delimiter = "") for directory in ctx.attrs.library_dirs] +
        extra_linker_flags
    )
    return [
        DefaultInfo(),
        CxxToolchainInfo(
            as_compiler_info = CCompilerInfo(
                compiler = _tool(ctx.attrs.c_compiler),
                compiler_type = compiler_type,
            ),
            asm_compiler_info = CCompilerInfo(
                compiler = _tool(ctx.attrs.c_compiler),
                compiler_type = compiler_type,
            ),
            binary_utilities_info = BinaryUtilitiesInfo(
                nm = _tool(prefix + "nm"),
                objcopy = _tool(prefix + "objcopy"),
                objdump = _tool(prefix + "objdump"),
                ranlib = _tool(prefix + "ranlib"),
                strip = _tool(prefix + "strip"),
            ),
            bolt_enabled = False,
            c_compiler_info = CCompilerInfo(
                compiler = _tool(ctx.attrs.c_compiler),
                compiler_type = compiler_type,
                compiler_flags = ctx.attrs.compiler_flags,
                preprocessor_flags = [],
                supports_content_based_paths = False,
                supports_two_phase_compilation = False,
            ),
            cpp_dep_tracking_mode = DepTrackingMode("show_headers"),
            cxx_compiler_info = CxxCompilerInfo(
                compiler = _tool(ctx.attrs.cxx_compiler),
                compiler_type = compiler_type,
                compiler_flags = ctx.attrs.compiler_flags,
                preprocessor_flags = [],
                supports_content_based_paths = False,
                supports_two_phase_compilation = False,
            ),
            header_mode = HeaderMode("symlink_tree_only"),
            internal_tools = ctx.attrs.internal_tools[CxxInternalTools],
            linker_info = LinkerInfo(
                archiver = _tool(prefix + ctx.attrs.archiver),
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
                linker = _tool(ctx.attrs.linker),
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
            llvm_link = _tool("llvm-link"),
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
        # Directories added to the linker's library search path.
        "library_dirs": attrs.list(attrs.source(allow_directory = True), default = []),
        # The Nix wrappers link through clang, which needs the toolchain's own
        # libraries on the search path.
        "link_toolchain_lib": attrs.bool(default = True),
        "linker": attrs.string(default = "clang++"),
        "linker_type": attrs.string(),
        "pic_behavior": attrs.string(),
        "platform_name": attrs.string(),
        "shared_library_name_format": attrs.string(),
        "shared_library_versioned_name_format": attrs.string(),
        # Prepended to the binutils and archiver names, for a target the host
        # binutils cannot handle.
        "tool_prefix": attrs.string(default = ""),
        "use_lld": attrs.bool(default = False),
        "internal_tools": attrs.default_only(attrs.exec_dep(
            providers = [CxxInternalTools],
            default = "prelude//cxx/tools:internal_tools",
        )),
    },
    is_toolchain_rule = True,
)

def nix_cxx_toolchain(name, os, visibility):
    """Declare the Nix-rooted C++ toolchain for one host operating system, or
    for wasm32 (`os = "none"`)."""
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
    else:
        fail("nix_cxx_toolchain does not support os {}".format(os))

def windows_gnu_cxx_toolchain(name, mingw_libraries, visibility):
    """The linker for x86_64-pc-windows-gnu: the Nix ld.lld, against the MinGW
    runtime and import libraries in `mingw_libraries`.

    Nothing built for that target compiles C or C++, so it has no compiler of
    its own.
    """
    hermetic_cxx_toolchain(
        name = name,
        archiver_supports_argfiles = True,
        compiler_flags = [],
        exec_compatible_with = WINDOWS_CROSS_EXEC_COMPATIBLE_WITH,
        library_dirs = [mingw_libraries],
        link_toolchain_lib = False,
        # rustc drives it as a GNU ld (`-Clinker-flavor=ld`), and lld reads the
        # `-m i386pep` rustc passes as a MinGW link.
        linker = "ld.lld",
        linker_type = "gnu",
        pic_behavior = "not_supported",
        platform_name = "windows-x86_64-gnu",
        shared_library_name_format = "{}.dll",
        shared_library_versioned_name_format = "{}.dll",
        tool_prefix = "llvm-",
        visibility = visibility,
    )

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
