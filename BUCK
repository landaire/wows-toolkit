CARGO_SRCS = glob([
    "Cargo.lock",
    "Cargo.toml",
    "assets/**",
    "crates/**",
    "embedded_resources/**",
    "flake.nix",
    "game_versions.toml",
    "mise.toml",
    "rust-toolchain.toml",
])

genrule(
    name = "cargo_binaries",
    srcs = CARGO_SRCS,
    outs = {
        "wows_toolkit": ["wows_toolkit"],
        "wowsunpack": ["wowsunpack"],
        "wows_data_mgr": ["wows_data_mgr"],
        "replayshark": ["replayshark"],
        "minimap_renderer": ["minimap_renderer"],
        "wgcheck": ["wgcheck"],
        "dhat_load": ["dhat_load"],
        "profile_replay": ["profile_replay"],
        "dhat_parse": ["dhat_parse"],
    },
    default_outs = ["wows_toolkit"],
    executable_outs = [
        "wows_toolkit",
        "wowsunpack",
        "wows_data_mgr",
        "replayshark",
        "minimap_renderer",
        "wgcheck",
        "dhat_load",
        "profile_replay",
        "dhat_parse",
    ],
    cmd = """
        WOWS_GAME_DATA=\"$BUCK_SCRATCH_PATH/no-game-data\" cargo build --locked --workspace --bins --target-dir \"$BUCK_SCRATCH_PATH/cargo-target\"
        WOWS_GAME_DATA=\"$BUCK_SCRATCH_PATH/no-game-data\" cargo build --locked --package wows_toolkit --bin dhat_load --features dhat-heap --target-dir \"$BUCK_SCRATCH_PATH/cargo-target\"
        WOWS_GAME_DATA=\"$BUCK_SCRATCH_PATH/no-game-data\" cargo build --locked --package wows_toolkit --bin profile_replay --features profile-bins --target-dir \"$BUCK_SCRATCH_PATH/cargo-target\"
        WOWS_GAME_DATA=\"$BUCK_SCRATCH_PATH/no-game-data\" cargo build --locked --package wowsunpack --bin dhat_parse --features dhat-heap --target-dir \"$BUCK_SCRATCH_PATH/cargo-target\"
        cp \"$BUCK_SCRATCH_PATH/cargo-target/debug/wows_toolkit\" \"$OUT/wows_toolkit\"
        cp \"$BUCK_SCRATCH_PATH/cargo-target/debug/wowsunpack\" \"$OUT/wowsunpack\"
        cp \"$BUCK_SCRATCH_PATH/cargo-target/debug/wows-data-mgr\" \"$OUT/wows_data_mgr\"
        cp \"$BUCK_SCRATCH_PATH/cargo-target/debug/replayshark\" \"$OUT/replayshark\"
        cp \"$BUCK_SCRATCH_PATH/cargo-target/debug/minimap_renderer\" \"$OUT/minimap_renderer\"
        cp \"$BUCK_SCRATCH_PATH/cargo-target/debug/wgcheck\" \"$OUT/wgcheck\"
        cp \"$BUCK_SCRATCH_PATH/cargo-target/debug/dhat_load\" \"$OUT/dhat_load\"
        cp \"$BUCK_SCRATCH_PATH/cargo-target/debug/profile_replay\" \"$OUT/profile_replay\"
        cp \"$BUCK_SCRATCH_PATH/cargo-target/debug/dhat_parse\" \"$OUT/dhat_parse\"
    """,
    cmd_exe = """
        set \"WOWS_GAME_DATA=%BUCK_SCRATCH_PATH%\\no-game-data\" && cargo build --locked --workspace --bins --target-dir \"%BUCK_SCRATCH_PATH%\\cargo-target\" && cargo build --locked --package wows_toolkit --bin dhat_load --features dhat-heap --target-dir \"%BUCK_SCRATCH_PATH%\\cargo-target\" && cargo build --locked --package wows_toolkit --bin profile_replay --features profile-bins --target-dir \"%BUCK_SCRATCH_PATH%\\cargo-target\" && cargo build --locked --package wowsunpack --bin dhat_parse --features dhat-heap --target-dir \"%BUCK_SCRATCH_PATH%\\cargo-target\" && copy /Y \"%BUCK_SCRATCH_PATH%\\cargo-target\\debug\\wows_toolkit.exe\" \"%OUT%\\wows_toolkit\" && copy /Y \"%BUCK_SCRATCH_PATH%\\cargo-target\\debug\\wowsunpack.exe\" \"%OUT%\\wowsunpack\" && copy /Y \"%BUCK_SCRATCH_PATH%\\cargo-target\\debug\\wows-data-mgr.exe\" \"%OUT%\\wows_data_mgr\" && copy /Y \"%BUCK_SCRATCH_PATH%\\cargo-target\\debug\\replayshark.exe\" \"%OUT%\\replayshark\" && copy /Y \"%BUCK_SCRATCH_PATH%\\cargo-target\\debug\\minimap_renderer.exe\" \"%OUT%\\minimap_renderer\" && copy /Y \"%BUCK_SCRATCH_PATH%\\cargo-target\\debug\\wgcheck.exe\" \"%OUT%\\wgcheck\" && copy /Y \"%BUCK_SCRATCH_PATH%\\cargo-target\\debug\\dhat_load.exe\" \"%OUT%\\dhat_load\" && copy /Y \"%BUCK_SCRATCH_PATH%\\cargo-target\\debug\\profile_replay.exe\" \"%OUT%\\profile_replay\" && copy /Y \"%BUCK_SCRATCH_PATH%\\cargo-target\\debug\\dhat_parse.exe\" \"%OUT%\\dhat_parse\"
    """,
)

alias(name = "wows_toolkit", actual = ":cargo_binaries[wows_toolkit]")
alias(name = "wowsunpack", actual = ":cargo_binaries[wowsunpack]")
alias(name = "wows_data_mgr", actual = ":cargo_binaries[wows_data_mgr]")
alias(name = "replayshark", actual = ":cargo_binaries[replayshark]")
alias(name = "minimap_renderer", actual = ":cargo_binaries[minimap_renderer]")
alias(name = "wgcheck", actual = ":cargo_binaries[wgcheck]")
alias(name = "dhat_load", actual = ":cargo_binaries[dhat_load]")
alias(name = "profile_replay", actual = ":cargo_binaries[profile_replay]")
alias(name = "dhat_parse", actual = ":cargo_binaries[dhat_parse]")

alias(name = "wgcheck_native", actual = "//crates/wgcheck:wgcheck_bin")
