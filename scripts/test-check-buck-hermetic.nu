#!/usr/bin/env nu

def main [] {
    let native_result = (do -i { ^nu scripts/check-buck-hermetic.nu //:wowsunpack } | complete)
    if $native_result.exit_code != 0 {
        error make "The hermetic action check rejected a native Buck closure."
    }

    for target in [
        "//tests/buck-hermetic:remote_action_alias",
        "//tests/buck-hermetic:ambient_environment_action_alias",
        "//tests/buck-hermetic:cache_path_action_alias",
        "//tests/buck-hermetic:cache_root_action_alias",
        "//tests/buck-hermetic:network_action_alias",
        "//tests/buck-hermetic:git_network_action_alias",
        "//tests/buck-hermetic:ssh_network_action_alias",
        "//tests/buck-hermetic:nc_network_action_alias",
        "//tests/buck-hermetic:bare_tool_action_alias",
    ] {
        let result = (do -i { ^nu scripts/check-buck-hermetic.nu $target } | complete)
        if $result.exit_code == 0 {
            error make $"The hermetic action check accepted fixture ($target)."
        }
        if not ($result.stderr | str contains "contains a prohibited action or environment") {
            error make $"The hermetic action check failed without identifying fixture ($target)."
        }
    }
}
