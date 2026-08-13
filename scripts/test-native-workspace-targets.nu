#!/usr/bin/env nu

def main [] {
    ^nu scripts/test-native-build-modes.nu
    ^nu scripts/test-buildscript-environment.nu
    ^nu scripts/test-workspace-package-metadata.nu

    let aliases = [
        "wows_toolkit"
        "wowsunpack"
        "wows_data_mgr"
        "replayshark"
        "minimap_renderer"
        "wgcheck"
        "dhat_load"
        "profile_replay"
        "dhat_parse"
    ]

    for alias in $aliases {
        let target = $"//:($alias)"
        let dep_query = (["deps(" $target ", 1)"] | str join)
        let deps = (do { ^buck2 uquery $dep_query } | complete)
        if $deps.exit_code != 0 {
            error make $"Buck query failed for ($target): ($deps.stderr)"
        }
        if ($deps.stdout | str contains "//:cargo_binaries") {
            error make $"($target) depends on the legacy cargo_binaries target."
        }

        let owner_query = (["kind('rust_binary', deps(" $target "))"] | str join)
        let owners = (do { ^buck2 cquery $owner_query } | complete)
        if $owners.exit_code != 0 {
            error make $"Buck configured query failed for ($target): ($owners.stderr)"
        }
        if ($owners.stdout | str trim | is-empty) {
            error make $"($target) has no native rust_binary owner."
        }

        ^buck2 build $target
        ^nu scripts/check-buck-hermetic.nu $target
    }

    for alias in ["wows_toolkit" "replayshark" "minimap_renderer"] {
        let target = $"//:($alias)"
        ^buck2 build -c native_build.mode=release $target
        ^nu scripts/check-buck-hermetic.nu $target release
    }
}
