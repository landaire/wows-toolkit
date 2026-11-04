load("@prelude//python_bootstrap:python_bootstrap.bzl", "PythonBootstrapToolchainInfo")
load("@toolchains//:hermetic_rust.bzl", "WasmToolsInfo")

def _platform_transition_impl(ctx):
    target = ctx.attrs.platform[PlatformInfo]

    def transition_impl(platform):
        # Only the target's own constraints (os and cpu) are replaced; the
        # build mode and any other constraint carry over.
        return PlatformInfo(
            label = target.label,
            configuration = ConfigurationInfo(
                constraints = platform.configuration.constraints | target.configuration.constraints,
                values = platform.configuration.values,
            ),
        )

    return [
        DefaultInfo(),
        TransitionInfo(impl = transition_impl),
    ]

# An incoming transition onto another platform's constraints.
platform_transition = rule(
    impl = _platform_transition_impl,
    attrs = {
        "platform": attrs.dep(providers = [PlatformInfo]),
    },
)

def wasm_target_compatible_with():
    """Constraints for a target that builds for wasm32.

    Only the Nix toolchain builds wasm32. Where the bootstrap has not published
    one (Windows), the wasm targets are incompatible, so patterns such as
    `root//...` skip them instead of failing analysis.
    """
    return [] if read_root_config("nix_toolchain", "root") else ["prelude//:none"]

def _wasm_site_impl(ctx):
    tools = ctx.attrs._wasm_tools[WasmToolsInfo]
    name = ctx.attrs.out_name
    module = ctx.attrs.module[DefaultInfo].default_outputs[0]

    bindgen = ctx.actions.declare_output("bindgen", dir = True)
    ctx.actions.run(
        cmd_args(
            tools.wasm_bindgen,
            "--target",
            "web",
            "--no-typescript",
            "--out-name",
            name,
            "--out-dir",
            bindgen.as_output(),
            module,
        ),
        category = "wasm_bindgen",
    )

    bindgen_wasm = bindgen.project(name + "_bg.wasm")
    if ctx.attrs.optimize:
        optimized = ctx.actions.declare_output(name + "_bg.wasm")
        ctx.actions.run(
            cmd_args(
                tools.wasm_opt,
                ctx.attrs.wasm_opt_flags,
                bindgen_wasm,
                "-o",
                optimized.as_output(),
            ),
            category = "wasm_opt",
        )
    else:
        optimized = bindgen_wasm

    site = ctx.actions.declare_output("site", dir = True)
    ctx.actions.run(
        cmd_args(
            ctx.attrs._python[PythonBootstrapToolchainInfo].interpreter,
            ctx.attrs._bundle_site,
            cmd_args(bindgen, format = "--bindgen={}"),
            cmd_args(optimized, format = "--wasm={}"),
            cmd_args(ctx.attrs.index, format = "--index={}"),
            "--name=" + name,
            "--public-url=" + ctx.attrs.public_url,
            cmd_args(site.as_output(), format = "--out={}"),
        ),
        category = "wasm_site",
    )

    return [DefaultInfo(default_output = site)]

wasm_site = rule(
    impl = _wasm_site_impl,
    doc = """A deployable directory for a wasm-bindgen web app.

    Runs wasm-bindgen with `--target web` over `module`, then wasm-opt with
    `wasm_opt_flags`, names the JS glue and the module after a hash of their
    contents, and writes `index` with a module script that loads them from
    under `public_url`.
    """,
    attrs = {
        "index": attrs.source(),
        # A wasm32 cdylib that wasm-bindgen can process.
        "module": attrs.dep(),
        # Release only, as trunk did.
        "optimize": attrs.bool(default = select({
            "toolchains//:release": True,
            "DEFAULT": False,
        })),
        "out_name": attrs.string(),
        "public_url": attrs.string(),
        "wasm_opt_flags": attrs.list(attrs.string()),
        "_bundle_site": attrs.source(default = "//build-support/buck/wasm:bundle_site.py"),
        "_python": attrs.toolchain_dep(
            default = "toolchains//:python_bootstrap",
            providers = [PythonBootstrapToolchainInfo],
        ),
        "_wasm_tools": attrs.toolchain_dep(
            default = "toolchains//:wasm_tools",
            providers = [WasmToolsInfo],
        ),
    },
)
