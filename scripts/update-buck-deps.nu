#!/usr/bin/env nu

^nix develop --command reindeer vendor
^nix develop --command reindeer buckify
rm -r -f third-party/rust/.cargo

let rustc = (^nix develop --command rustc --version | parse -r 'rustc 1\.(?<minor>\d+)\.(?<patch>\d+)' | first)
$"crate::version::Version {
    minor: ($rustc.minor),
    patch: ($rustc.patch),
    channel: crate::version::Channel::Stable,
}
" | save -f third-party/rust/fixups/rustversion/version.expr

let download_rules = (^rg -n '^\s*(http_(archive|file)|git_(fetch|repository))\(' third-party/rust/BUCK | complete)
if $download_rules.exit_code == 0 {
    error make "Reindeer generated a network download rule; vendoring is incomplete."
}
