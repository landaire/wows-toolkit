def _fixture_action_impl(ctx):
    output = ctx.actions.declare_output(ctx.label.name + ".txt")
    ctx.actions.write(output, ctx.attrs.content)
    return [DefaultInfo(default_output = output)]

fixture_action = rule(
    impl = _fixture_action_impl,
    attrs = {
        "content": attrs.string(),
    },
)
