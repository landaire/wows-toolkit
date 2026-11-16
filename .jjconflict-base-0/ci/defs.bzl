"""Rules that put the repository's own checks behind buck2, so CI runs only
buck2 commands."""

def _python():
    path = read_root_config("hermetic_tools", "python")
    if path == None:
        fail("Missing [hermetic_tools] python. Run ./bootstrap.sh (or bootstrap.ps1 on Windows) before invoking Buck2.")
    return path

def _script_test_impl(ctx: AnalysisContext) -> list[Provider]:
    command = [_python(), ctx.attrs.script] + ctx.attrs.args
    return [
        DefaultInfo(),
        ExternalRunnerTestInfo(
            type = "custom",
            command = command,
            # The checks read the repository's own files.
            run_from_project_root = True,
            use_project_relative_paths = True,
        ),
    ]

# A Python check over files in the repository, run by `buck2 test`.
script_test = rule(
    impl = _script_test_impl,
    attrs = {
        "args": attrs.list(attrs.string(), default = []),
        "script": attrs.source(),
    },
)

def _nu_script_impl(ctx: AnalysisContext) -> list[Provider]:
    # nu comes from the Nix devShell CI runs in. These scripts drive buck2
    # themselves, which only `buck2 run` allows: it starts them after its own
    # command has finished.
    return [
        DefaultInfo(),
        RunInfo(args = cmd_args("nu", "--no-config-file", ctx.attrs.script, ctx.attrs.args)),
    ]

# A nu script that queries or builds with buck2, run by `buck2 run`.
nu_script = rule(
    impl = _nu_script_impl,
    attrs = {
        "args": attrs.list(attrs.string(), default = []),
        "script": attrs.source(),
    },
)
