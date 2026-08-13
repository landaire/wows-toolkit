genrule(
    name = "wows_toolkit",
    out = "wows_toolkit",
    srcs = glob([
        "Cargo.lock",
        "Cargo.toml",
        "assets/**",
        "crates/**",
        "embedded_resources/**",
        "flake.nix",
        "game_versions.toml",
        "mise.toml",
        "rust-toolchain.toml",
    ]),
    executable = True,
    cmd = """
        WOWS_GAME_DATA=\"$TMP/no-game-data\" cargo build --locked --package wows_toolkit --bin wows_toolkit --target-dir \"$TMP/cargo-target\"
        cp \"$TMP/cargo-target/debug/wows_toolkit\" \"$OUT\"
    """,
    cmd_exe = """
        set \"WOWS_GAME_DATA=%TMP%\\no-game-data\" && cargo build --locked --package wows_toolkit --bin wows_toolkit --target-dir \"%TMP%\\cargo-target\" && copy /Y \"%TMP%\\cargo-target\\debug\\wows_toolkit.exe\" \"%OUT%\"
    """,
)
