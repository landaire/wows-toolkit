#!/usr/bin/env nu

# Checks the runpaths of what the given root aliases, or every one, build in
# `native_build_mode`.
def main [native_build_mode: string, ...aliases: string] {
    let targets = if ($aliases | is-empty) { ^buck2 uquery "kind(alias, root//:)" | lines } else { $aliases }
    for alias in $targets {
        ^nu --no-config-file scripts/check-buck-runpath.nu $alias $native_build_mode
    }
}
