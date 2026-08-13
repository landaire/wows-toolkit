#!/usr/bin/env nu

^nix develop --command reindeer vendor
^nix develop --command reindeer buckify
rm -rf third-party/rust/.cargo

let download_rules = (^rg -n 'http_archive\(|git_fetch\(' third-party/rust/BUCK | complete)
if $download_rules.exit_code == 0 {
    error make "Reindeer generated a network download rule; vendoring is incomplete."
}
