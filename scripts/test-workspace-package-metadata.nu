#!/usr/bin/env nu

def main [] {
    let cargo_version = (open crates/wgcheck/Cargo.toml | get package.version)
    let buckfile = (open crates/wgcheck/BUCK)
    let versions = ($buckfile | lines | where {|line| $line | str starts-with "    version = "})

    if ($versions | length) != 2 or ($versions | any {|line| $line != $"    version = \"($cargo_version)\","}) {
        error make {msg: "wgcheck Buck package metadata does not match Cargo.toml."}
    }
}
