load(":hermetic_msvc.bzl", "WindowsToolPathsInfo")

def _wix_msi_impl(ctx):
    tools = ctx.attrs.tools[WindowsToolPathsInfo]
    if not tools.wix or not tools.wix_ui_extension or not tools.wix_util_extension:
        fail("WiX is not provisioned. Run toolchains/windows/provision-toolchain.ps1, which writes the wix entries into .buckconfig.local.")

    # One directory, so -bindpath is the directory itself rather than something
    # derived from whichever payload entry happened to be first.
    contents = dict(ctx.attrs.binaries)
    contents.update(ctx.attrs.pdbs)
    payload = ctx.actions.copied_dir("payload", contents, has_content_based_path = False)

    output = ctx.actions.declare_output(ctx.attrs.out, has_content_based_path = False)
    command = cmd_args(
        tools.wix,
        "build",
        ctx.attrs.wxs,
        "-bindpath",
        payload,
        # wix.exe takes the switch and the assignment as separate arguments.
        "-d",
        cmd_args(payload, format = "BinDir={}"),
        "-d",
        cmd_args(ctx.attrs.version, format = "Version={}"),
        # WiX loads an extension assembly; the .nupkg it ships in is just a zip.
        "-ext",
        tools.wix_ui_extension,
        "-ext",
        tools.wix_util_extension,
        "-o",
        output.as_output(),
        hidden = ctx.attrs.assets,
    )
    ctx.actions.run(
        command,
        category = "wix_unsigned_msi",
        env = {"PATH": ""},
    )
    return [DefaultInfo(default_output = output)]

wix_msi = rule(
    impl = _wix_msi_impl,
    attrs = {
        "assets": attrs.list(attrs.source(), default = []),
        "binaries": attrs.dict(key = attrs.string(), value = attrs.source()),
        "out": attrs.string(),
        "pdbs": attrs.dict(key = attrs.string(), value = attrs.source(), default = {}),
        "version": attrs.string(),
        "tools": attrs.exec_dep(providers = [WindowsToolPathsInfo]),
        "wxs": attrs.list(attrs.source()),
    },
)
