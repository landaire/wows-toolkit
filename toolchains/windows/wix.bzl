load(":hermetic_msvc.bzl", "WindowsToolPathsInfo")

def _wix_msi_impl(ctx):
    tools = ctx.attrs.tools[WindowsToolPathsInfo]
    payload = []
    for name, binary in ctx.attrs.binaries.items():
        payload.append(ctx.actions.copy_file("payload/" + name, binary, has_content_based_path = False))
    for name, pdb in ctx.attrs.pdbs.items():
        payload.append(ctx.actions.copy_file("payload/" + name, pdb, has_content_based_path = False))

    output = ctx.actions.declare_output(ctx.attrs.out, has_content_based_path = False)
    command = cmd_args(
        tools.wix,
        "build",
        ctx.attrs.wxs,
        "-bindpath",
        cmd_args(payload[0].dirname, format = "{}"),
        "-dBinDir=" + payload[0].dirname,
        "-ext",
        tools.wix_extensions + "/WixToolset.UI.wixext.6.0.2.nupkg",
        "-ext",
        tools.wix_extensions + "/WixToolset.Util.wixext.6.0.2.nupkg",
        "-o",
        output.as_output(),
        hidden = payload + ctx.attrs.assets + [ctx.attrs.version_input],
    )
    ctx.actions.run(
        command,
        category = "wix_unsigned_msi",
        env = {"PATH": "", "SOURCE_DATE_EPOCH": ctx.attrs.source_date_epoch},
    )
    return [DefaultInfo(default_output = output)]

wix_msi = rule(
    impl = _wix_msi_impl,
    attrs = {
        "assets": attrs.list(attrs.source(), default = []),
        "binaries": attrs.dict(key = attrs.string(), value = attrs.source()),
        "out": attrs.string(),
        "pdbs": attrs.dict(key = attrs.string(), value = attrs.source(), default = {}),
        "source_date_epoch": attrs.string(default = "0"),
        "version_input": attrs.source(),
        "tools": attrs.exec_dep(providers = [WindowsToolPathsInfo]),
        "wxs": attrs.list(attrs.source()),
    },
)
