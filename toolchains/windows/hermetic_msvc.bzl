load("@prelude//rust:rust_toolchain.bzl", "PanicRuntime", "RustToolchainInfo")
load("@prelude//toolchains:cxx.bzl", "CxxToolsInfo")
load("@prelude//cxx:cxx_toolchain_types.bzl", "LinkerType")

WindowsToolPathsInfo = provider(fields = {
    "root": provider_field(typing.Any),
    "cl": provider_field(typing.Any),
    "lib": provider_field(typing.Any),
    "link": provider_field(typing.Any),
    "rc": provider_field(typing.Any),
    "midl": provider_field(typing.Any),
    "ml64": provider_field(typing.Any),
    "rustc": provider_field(typing.Any),
    "rustdoc": provider_field(typing.Any),
    "nasm": provider_field(typing.Any),
    "wix": provider_field(typing.Any),
})

def _tool(root, relative_path):
    return cmd_args(root, "/" + relative_path, delimiter = "")

def _native_build_mode():
    mode = read_root_config("native_build", "mode", "debug")
    if mode not in ["debug", "release"]:
        fail("native_build.mode must be debug or release, got {}".format(mode))
    return mode

def _rustc_flags():
    if _native_build_mode() == "release":
        return ["-Copt-level=3", "-Cdebuginfo=0"]
    return ["-Copt-level=0", "-Cdebuginfo=2"]

def _rustc_env(root):
    return {
        "PATH": _tool(root, "VisualStudio/BuildTools/VC/Tools/MSVC/14.44.37537/bin/Hostx64/x64"),
        "INCLUDE": _tool(root, "VisualStudio/BuildTools/VC/Tools/MSVC/14.44.37537/include"),
        "LIB": _tool(root, "VisualStudio/BuildTools/VC/Tools/MSVC/14.44.37537/lib/x64"),
        "LIBPATH": _tool(root, "VisualStudio/BuildTools/VC/Tools/MSVC/14.44.37537/lib/x64"),
    }

def _wrapper(ctx, name, executable):
    root = ctx.attrs.toolchain_root
    content = cmd_args(
        "@echo off\r\nsetlocal DisableDelayedExpansion\r\nset \"PATH=\"\r\nset \"INCLUDE=",
        _tool(root, "VisualStudio/BuildTools/VC/Tools/MSVC/14.44.37537/include"), ";",
        _tool(root, "WindowsKits/10/Include/10.0.26100.0/ucrt"), ";",
        _tool(root, "WindowsKits/10/Include/10.0.26100.0/um"), ";",
        _tool(root, "WindowsKits/10/Include/10.0.26100.0/shared"),
        "\"\r\nset \"LIB=",
        _tool(root, "VisualStudio/BuildTools/VC/Tools/MSVC/14.44.37537/lib/x64"), ";",
        _tool(root, "WindowsKits/10/Lib/10.0.26100.0/ucrt/x64"), ";",
        _tool(root, "WindowsKits/10/Lib/10.0.26100.0/um/x64"),
        "\"\r\nset \"LIBPATH=%LIB%\"\r\n\"", executable, "\" %*\r\n",
        delimiter = "",
    )
    wrapper, _ = ctx.actions.write(name + ".bat", content, allow_args = True)
    return wrapper

def _msvc_tools_impl(ctx):
    root = ctx.attrs.toolchain_root
    cl = _wrapper(ctx, "cl", _tool(root, "VisualStudio/BuildTools/VC/Tools/MSVC/14.44.37537/bin/Hostx64/x64/cl.exe"))
    lib = _wrapper(ctx, "lib", _tool(root, "VisualStudio/BuildTools/VC/Tools/MSVC/14.44.37537/bin/Hostx64/x64/lib.exe"))
    link = _wrapper(ctx, "link", _tool(root, "VisualStudio/BuildTools/VC/Tools/MSVC/14.44.37537/bin/Hostx64/x64/link.exe"))
    rc = _wrapper(ctx, "rc", _tool(root, "WindowsKits/10/bin/10.0.26100.0/x64/rc.exe"))
    ml64 = _wrapper(ctx, "ml64", _tool(root, "VisualStudio/BuildTools/VC/Tools/MSVC/14.44.37537/bin/Hostx64/x64/ml64.exe"))
    rustc = _tool(root, "Rust/1.92.0/bin/rustc.exe")
    rustdoc = _tool(root, "Rust/1.92.0/bin/rustdoc.exe")
    nasm = _tool(root, "NASM/3.01/nasm.exe")
    wix = _tool(root, "WiX/6.0.2/wix.exe")
    return [
        DefaultInfo(),
        RunInfo(args = [wix]),
        CxxToolsInfo(
            compiler = cl, compiler_type = "windows", cxx_compiler = cl,
            asm_compiler = ml64, asm_compiler_type = "windows_ml64", rc_compiler = rc,
            cvtres_compiler = _wrapper(ctx, "cvtres", _tool(root, "WindowsKits/10/bin/10.0.26100.0/x64/cvtres.exe")),
            archiver = lib, archiver_type = "windows", linker = link, linker_type = LinkerType("windows"),
        ),
        WindowsToolPathsInfo(root = root, cl = cl, lib = lib, link = link, rc = rc, midl = _tool(root, "WindowsKits/10/bin/10.0.26100.0/x64/midl.exe"), ml64 = ml64, rustc = rustc, rustdoc = rustdoc, nasm = nasm, wix = wix),
    ]

hermetic_msvc_tools = rule(impl = _msvc_tools_impl, attrs = {"toolchain_root": attrs.source(allow_directory = True)})

def _hermetic_msvc_rust_toolchain_impl(ctx):
    tools = ctx.attrs.tools[WindowsToolPathsInfo]
    return [DefaultInfo(), RustToolchainInfo(
        allow_lints = [], clippy_driver = RunInfo(args = [tools.rustc]), clippy_toml = None,
        compiler = RunInfo(args = [tools.rustc]), default_edition = ctx.attrs.default_edition,
        deny_lints = [], doctests = False, nightly_features = False, panic_runtime = PanicRuntime("unwind"),
        report_unused_deps = False, rustc_binary_flags = [], rustc_env = _rustc_env(tools.root), rustc_flags = _rustc_flags(),
        rustc_target_triple = "x86_64-pc-windows-msvc", rustc_test_flags = [],
        rustdoc = RunInfo(args = [tools.rustdoc]), rustdoc_flags = [], warn_lints = [],
    )]

hermetic_msvc_rust_toolchain = rule(
    impl = _hermetic_msvc_rust_toolchain_impl,
    attrs = {"default_edition": attrs.string(), "tools": attrs.dep(providers = [WindowsToolPathsInfo])},
    is_toolchain_rule = True,
)
