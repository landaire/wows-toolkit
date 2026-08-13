#!/usr/bin/env nu

def main [target: string] {
    let actions = ($nu.temp-dir | path join $"buck-hermetic-($nu.pid).actions")
    try {
        let query = (["all_actions(deps(" $target "))"] | str join)
        ^buck2 aquery $query --output-all-attributes o> $actions
        let prohibited = (^rg -n '(^|[^[:alnum:]_])cargo([^[:alnum:]_]|$)|http_(archive|file)\(|git_(fetch|repository)\(|https?://' $actions | complete)
        if $prohibited.exit_code == 0 {
            error make $"Buck action graph for ($target) contains a Cargo executable or download action."
        }
    } finally {
        rm -f $actions
    }
}
