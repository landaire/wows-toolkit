#!/usr/bin/env nu

def main [target: string, native_build_mode = "debug"] {
    let actions = ($nu.temp-dir | path join $"buck-hermetic-($nu.pid).actions")
    try {
        let query = (["all_actions(deps(" $target "))"] | str join)
        if $native_build_mode == "release" {
            ^buck2 aquery -c native_build.mode=release $query --output-all-attributes o> $actions
        } else if $native_build_mode == "debug" {
            ^buck2 aquery $query --output-all-attributes o> $actions
        } else {
            error make $"Unsupported native build mode: ($native_build_mode)"
        }
        let prohibited = (^rg --case-sensitive -n '(^|[[:space:]"])([^[:space:]"]+/)?cargo([[:space:]"]|$)|http_(archive|file)\(|git_(fetch|repository)\(' $actions | complete)
        if $prohibited.exit_code == 0 {
            error make $"Buck action graph for ($target) contains a Cargo executable or download action."
        }
    } finally {
        rm -f $actions
    }
}
