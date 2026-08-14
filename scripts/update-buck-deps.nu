#!/usr/bin/env nu

^nix develop --command reindeer vendor
^nix develop --command reindeer buckify
rm -r -f third-party/rust/.cargo

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

let rustc = (^nix develop --command rustc --version | parse -r 'rustc 1\.(?<minor>\d+)\.(?<patch>\d+)' | first)
$"crate::version::Version {
    minor: ($rustc.minor),
    patch: ($rustc.patch),
    channel: crate::version::Channel::Stable,
}
" | save -f third-party/rust/fixups/rustversion/version.expr

let download_rules = (
    open --raw third-party/rust/BUCK
    | lines
    | where $it =~ '^\s*(http_(archive|file)|git_(fetch|repository))\('
)
if ($download_rules | is-not-empty) {
    print -e ($download_rules | first 10 | str join "\n")
    error make {msg: "Reindeer generated a network download rule; vendoring is incomplete."}
}
