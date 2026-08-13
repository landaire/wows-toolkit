#!/usr/bin/env nu

def main [target: string] {
    let actions = (^mktemp | str trim)
    try {
        ^buck2 aquery $target --output-all-attributes o> $actions
        let prohibited = (^rg -n '(^|[^[:alnum:]_])cargo([^[:alnum:]_]|$)|http_(archive|file)\(|git_(fetch|repository)\(|https?://' $actions | complete)
        if $prohibited.exit_code == 0 {
            error make $"Buck action graph for ($target) contains a Cargo executable or download action."
        }
    } finally {
        rm -f $actions
    }
}
