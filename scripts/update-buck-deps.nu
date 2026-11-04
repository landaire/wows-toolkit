#!/usr/bin/env nu

# reindeer and rustc come from PATH: reindeer from `mise run install-reindeer`,
# rustc from the rustup shim. Going through `nix develop` restricted this to
# Linux and macOS, so Windows had to run it under WSL.
#
# Nothing is vendored: the generated rules download each crate, checked against
# Cargo.lock, and fetch each git dependency at its locked commit.
^reindeer buckify

# Buck spills long command lines into a line-oriented args file, so an argument
# containing a newline swallows every argument after it. Many crates describe
# themselves across several lines. This only shows up where command lines are
# short enough to force the spill, which in practice means Windows.
let flattened = (
    open --raw third-party/rust/BUCK
    | lines
    | each {|line|
        if ($line =~ '"CARGO_PKG_(DESCRIPTION|AUTHORS)": ') {
            $line | str replace --all --regex '\\[rn]' ' '
        } else {
            $line
        }
    }
    | str join "\n"
)
$"($flattened)\n" | save -f third-party/rust/BUCK

let rustc = (^rustc --version | parse -r 'rustc 1\.(?<minor>\d+)\.(?<patch>\d+)' | first)
$"crate::version::Version {
    minor: ($rustc.minor),
    patch: ($rustc.patch),
    channel: crate::version::Channel::Stable,
}
" | save -f third-party/rust/fixups/rustversion/version.expr
