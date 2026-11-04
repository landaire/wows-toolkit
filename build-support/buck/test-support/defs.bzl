def _constraint_transition_impl(ctx):
    constraint = ctx.attrs.constraint_value[ConstraintValueInfo]

    def transition_impl(platform):
        return PlatformInfo(
            label = platform.label,
            configuration = ConfigurationInfo(
                constraints = platform.configuration.constraints | {
                    constraint.setting.label: constraint,
                },
                values = platform.configuration.values,
            ),
        )

    return [
        DefaultInfo(),
        TransitionInfo(impl = transition_impl),
    ]

# An incoming transition that adds one constraint value to the platform.
constraint_transition = rule(
    impl = _constraint_transition_impl,
    attrs = {
        "constraint_value": attrs.dep(providers = [ConstraintValueInfo]),
    },
)
