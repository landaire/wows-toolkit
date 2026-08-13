#!/usr/bin/env nu

def main [] {
    let result = (do -i { ^nu scripts/check-buck-hermetic.nu //tests/buck-hermetic:remote_action_alias } | complete)
    if $result.exit_code == 0 {
        error make "The hermetic action check accepted a remote-rule fixture."
    }
    if not ($result.stderr | str contains "contains a Cargo executable or download action") {
        error make "The hermetic action check failed without identifying the remote-rule fixture."
    }
}
