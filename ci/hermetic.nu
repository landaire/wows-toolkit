#!/usr/bin/env nu

# Checks the action graph of the given root aliases, or of every one, in both
# build modes.
def main [...aliases: string] {
    let targets = if ($aliases | is-empty) { ^buck2 targets root//: | lines } else { $aliases }
    for alias in $targets {
        ^nu --no-config-file scripts/check-buck-hermetic.nu $alias
        ^nu --no-config-file scripts/check-buck-hermetic.nu $alias release
    }
}
