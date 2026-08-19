load("@prelude//rust:rust_toolchain.bzl", "PanicRuntime", "RustToolchainInfo")
load("@prelude//toolchains:cxx.bzl", "CxxToolsInfo")
load("@prelude//cxx:cxx_toolchain_types.bzl", "LinkerType")

WindowsToolPathsInfo = provider(fields = {
    "cl": provider_field(typing.Any),
    "lib": provider_field(typing.Any),
    "link": provider_field(typing.Any),
    "rc": provider_field(typing.Any),
    "midl": provider_field(typing.Any),
    "ml64": provider_field(typing.Any),
    "rustc": provider_field(typing.Any),
    "rustdoc": provider_field(typing.Any),
    "clippy_driver": provider_field(typing.Any),
    "rustfmt": provider_field(typing.Any),
    "nasm": provider_field(typing.Any),
    "wix": provider_field(typing.Any),
    "wix_ui_extension": provider_field(typing.Any),
    "wix_util_extension": provider_field(typing.Any),
})

# Every tool is an absolute path published by toolchains/windows/verify-toolchain.ps1,
# the same mechanism scripts/refresh-buck-toolchain.nu uses on macOS and Linux.
# Taking the toolchain as a source directory instead would make Buck digest the
# whole multi-gigabyte Visual Studio and SDK tree on every build.
def _tool(name, optional = False):
    path = read_root_config("hermetic_tools", name)
    if path == None:
        if optional:
            # Only the installer needs it, so let everything else configure.
            return ""
        fail("Missing [hermetic_tools] {}. Run toolchains/windows/verify-toolchain.ps1 before invoking Buck2.".format(name))
    return path

def _native_build_mode():
    mode = read_root_config("native_build", "mode", "debug")
    if mode not in ["debug", "release"]:
        fail("native_build.mode must be debug or release, got {}".format(mode))
    return mode

def _rustc_flags():
    # No -Clinker here: the prelude already points rustc at the linker from
    # CxxToolsInfo, which is the pinned link.exe wrapper.
    if _native_build_mode() == "release":
        return ["-Copt-level=3", "-Cdebuginfo=0"]
    return ["-Copt-level=0", "-Cdebuginfo=2"]

def _wrapper(ctx, name, executable):
    # cl.exe and link.exe read their search paths from the environment. The
    # wrapper sets exactly those and clears PATH, so nothing is inherited.
    content = cmd_args(
        "@echo off\r\nsetlocal DisableDelayedExpansion\r\nset \"PATH=\"\r\nset \"INCLUDE=",
        ctx.attrs.include,
        "\"\r\nset \"LIB=",
        ctx.attrs.lib_paths,
        "\"\r\nset \"LIBPATH=%LIB%\"\r\n\"", executable, "\" %*\r\n",
        delimiter = "",
    )
    wrapper, _ = ctx.actions.write(name + ".bat", content, allow_args = True)
    return wrapper

def _msvc_tools_impl(ctx):
    cl = _wrapper(ctx, "cl", ctx.attrs.cc)
    lib = _wrapper(ctx, "lib", ctx.attrs.ar)
    link = _wrapper(ctx, "link", ctx.attrs.link)
    rc = _wrapper(ctx, "rc", ctx.attrs.rc)
    ml64 = _wrapper(ctx, "ml64", ctx.attrs.ml64)
    cvtres = _wrapper(ctx, "cvtres", ctx.attrs.cvtres)
    wix = ctx.attrs.wix
    return [
        DefaultInfo(),
        RunInfo(args = [wix]),
        CxxToolsInfo(
            compiler = cl,
            compiler_type = "windows",
            cxx_compiler = cl,
            asm_compiler = ml64,
            asm_compiler_type = "windows_ml64",
            rc_compiler = rc,
            cvtres_compiler = cvtres,
            archiver = lib,
            archiver_type = "windows",
            linker = link,
            linker_type = LinkerType("windows"),
        ),
        WindowsToolPathsInfo(
            cl = cl,
            lib = lib,
            link = link,
            rc = rc,
            midl = ctx.attrs.midl,
            ml64 = ml64,
            rustc = ctx.attrs.rustc,
            rustdoc = ctx.attrs.rustdoc,
            clippy_driver = ctx.attrs.clippy_driver,
            rustfmt = ctx.attrs.rustfmt,
            nasm = ctx.attrs.nasm,
            wix = wix,
            wix_ui_extension = ctx.attrs.wix_ui_extension,
            wix_util_extension = ctx.attrs.wix_util_extension,
        ),
    ]

# clang and llvm_ar are deliberately absent: verify-toolchain.ps1 publishes them
# for the Cargo wasm32 build, but no Buck rule consumes them yet, and requiring
# them here would fail package loading for everyone who does not need them.
_TOOL_ATTRS = ["ar", "cc", "clippy_driver", "cvtres", "include", "lib_paths", "link", "midl", "ml64", "nasm", "rc", "rustc", "rustdoc", "rustfmt"]

# WiX is needed by the MSI target alone. Requiring it at package load would stop
# every Windows target configuring on a machine that never builds an installer.
_WIX_ATTRS = ["wix", "wix_ui_extension", "wix_util_extension"]

_hermetic_msvc_tools_rule = rule(
    impl = _msvc_tools_impl,
    attrs = {name: attrs.string() for name in _TOOL_ATTRS + _WIX_ATTRS},
)

def hermetic_msvc_tools(name, visibility):
    # read_root_config is unavailable during analysis, so the paths are resolved
    # here, while the package is loading, and passed in as attributes.
    _hermetic_msvc_tools_rule(
        name = name,
        visibility = visibility,
        **{attr: _tool(attr, attr in _WIX_ATTRS) for attr in _TOOL_ATTRS + _WIX_ATTRS}
    )

def _hermetic_msvc_rust_toolchain_impl(ctx):
    tools = ctx.attrs.tools[WindowsToolPathsInfo]
    return [DefaultInfo(), RustToolchainInfo(
        allow_lints = [],
        # Not tools.rustc: pointing this at rustc makes every [clippy.txt] and
        # [clippy.json] sub-target produce an empty diagnostic file, so the
        # whole lint surface silently passes on Windows.
        clippy_driver = RunInfo(args = [tools.clippy_driver]),
        clippy_toml = None,
        compiler = RunInfo(args = [tools.rustc]),
        default_edition = ctx.attrs.default_edition,
        deny_lints = [],
        doctests = False,
        nightly_features = False,
        panic_runtime = PanicRuntime("unwind"),
        report_unused_deps = False,
        rustc_binary_flags = [],
        # INCLUDE and LIB have no rustc flag and must be passed as environment.
        rustc_env = {
            "INCLUDE": ctx.attrs.include,
            "LIB": ctx.attrs.lib_paths,
            "LIBPATH": ctx.attrs.lib_paths,
        },
        rustc_flags = ctx.attrs.rustc_flags,
        rustc_target_triple = "x86_64-pc-windows-msvc",
        rustc_test_flags = [],
        rustdoc = RunInfo(args = [tools.rustdoc]),
        rustdoc_flags = [],
        warn_lints = [],
    )]

_hermetic_msvc_rust_toolchain_rule = rule(
    impl = _hermetic_msvc_rust_toolchain_impl,
    attrs = {
        "default_edition": attrs.string(),
        "include": attrs.string(),
        "lib_paths": attrs.string(),
        "rustc_flags": attrs.list(attrs.string()),
        "tools": attrs.dep(providers = [WindowsToolPathsInfo]),
    },
    is_toolchain_rule = True,
)

def hermetic_msvc_rust_toolchain(name, default_edition, tools, visibility):
    # Resolved while the package loads; read_root_config is unavailable during
    # analysis.
    _hermetic_msvc_rust_toolchain_rule(
        name = name,
        default_edition = default_edition,
        include = _tool("include"),
        lib_paths = _tool("lib_paths"),
        rustc_flags = _rustc_flags(),
        tools = tools,
        visibility = visibility,
    )

def _windows_resource_impl(ctx):
    tools = ctx.attrs.tools[WindowsToolPathsInfo]
    output = ctx.actions.declare_output(ctx.attrs.out)

    # rc.exe resolves #include and the icon relative to the script, so the script
    # and every file it names must sit in one directory.
    sources = ctx.actions.symlinked_dir(
        "rc_sources",
        {src.short_path: src for src in [ctx.attrs.src] + ctx.attrs.resources},
    )

    ctx.actions.run(
        cmd_args(
            tools.rc,
            "/nologo",
            cmd_args(output.as_output(), format = "/fo{}"),
            cmd_args(sources, format = "{}/" + ctx.attrs.src.short_path),
            hidden = [sources],
        ),
        category = "windows_resource",
        env = {"PATH": ""},
    )
    return [DefaultInfo(default_output = output)]

windows_resource = rule(
    impl = _windows_resource_impl,
    attrs = {
        "out": attrs.string(),
        "resources": attrs.list(attrs.source(), default = []),
        "src": attrs.source(),
        "tools": attrs.exec_dep(providers = [WindowsToolPathsInfo]),
    },
)
