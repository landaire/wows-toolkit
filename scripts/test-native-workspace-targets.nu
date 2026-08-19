#!/usr/bin/env nu

def main [] {
    ^nu scripts/test-native-build-modes.nu
    ^nu scripts/test-buildscript-environment.nu
    ^nu scripts/test-workspace-package-metadata.nu

    # Enumerated from the root BUCK file rather than repeated here, so a new
    # alias is covered without touching this test.
    let targets = (^buck2 targets "root//:" | lines | where {|line| ($line | str trim) != "" })
    if ($targets | length) < 5 {
        error make {msg: $"Expected the root package to declare the release aliases, found ($targets | length)."}
    }

    for target in $targets {
        let dep_query = (["deps(" $target ", 1)"] | str join)
        let deps = (do { ^buck2 uquery $dep_query } | complete)
        if $deps.exit_code != 0 {
            error make {msg: $"Buck query failed for ($target): ($deps.stderr)"}
        }
        if ($deps.stdout | str contains "//:cargo_binaries") {
            error make {msg: $"($target) depends on the legacy cargo_binaries target."}
        }

        let owner_query = (["kind('rust_binary', deps(" $target "))"] | str join)
        let owners = (do { ^buck2 cquery $owner_query } | complete)
        if $owners.exit_code != 0 {
            error make {msg: $"Buck configured query failed for ($target): ($owners.stderr)"}
        }
        if ($owners.stdout | str trim | is-empty) {
            error make {msg: $"($target) has no native rust_binary owner."}
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
