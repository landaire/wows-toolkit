#!/usr/bin/env nu

# Checks the action graph of every root alias in both build modes.
def main [] {
    for alias in (^buck2 targets root//: | lines) {
        ^nu --no-config-file scripts/check-buck-hermetic.nu $alias
        ^nu --no-config-file scripts/check-buck-hermetic.nu $alias release
    }
}
