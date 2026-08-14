def windows_x86_64_msvc_platform(name, visibility):
    native.platform(
        name = name,
        constraint_values = [
            "config//os/constraints:windows",
            "config//cpu/constraints:x86_64",
            "prelude//abi/constraints:msvc",
        ],
        visibility = visibility,
    )
