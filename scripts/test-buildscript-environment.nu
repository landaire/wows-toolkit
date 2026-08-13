#!/usr/bin/env nu

def main [] {
    let hostile_marker = "/hostile-buildscript-environment"
    let buck2 = (which buck2 | get 0.path)
    let actions = (
        with-env {
            PATH: $"($hostile_marker)/path:($env.PATH)"
            CARGO_HOME: $"($hostile_marker)/cargo-home"
            SCCACHE_DIR: $"($hostile_marker)/sccache"
            XDG_CACHE_HOME: $"($hostile_marker)/cache"
        } {
            run-external $buck2 aquery 'all_actions(deps(//:wowsunpack))' "--output-all-attributes"
        }
    )

    if ($actions | str contains $hostile_marker) {
        error make "A hostile inherited environment value reached a build-script action."
    }
}
