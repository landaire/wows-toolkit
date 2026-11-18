#!/usr/bin/env nu

# Shorthand for the commands this repository runs often. Run it with no
# arguments, or with --help, to list them.
#
# Buck is the gate: CI builds every alias with it and every release ships from
# that graph, so a change that builds under Cargo but not under Buck does not
# land. The cargo-* commands are the escape hatch for what Buck cannot do here,
# autofix and doctests, and for fast iteration. `root//:` is every alias in the
# root BUCK file, so nothing has to repeat the list when one is added.

const ROOT = path self ..

# The crates published to crates.io, which `bump` moves together.
const PUBLISHED = [
    wows-core
    wowsunpack
    wows_replays
    wows_minimap_renderer
    wows-battle-world
    wows-data-mgr
    wows-replay-insights
    wt-translations
]

# Runs `command` from the repository root with the repository's tools first on
# PATH: the pinned buck2 in bin, and the NASM `setup` installs on Windows. The
# variables `setup` writes to .tooling/dev.env are set too.
def in-repo [command: closure] {
    let dirs = [
        ($ROOT | path join bin)
        ($ROOT | path join .tooling nasm)
        'C:/Program Files/NASM'
    ] | where {|dir| $dir | path exists }
    let dev_env = ($ROOT | path join .tooling dev.env)
    let variables = if ($dev_env | path exists) {
        open --raw $dev_env | lines | where {|line| $line =~ '=' } | parse '{key}={value}' | reduce --fold {} {|row, all| $all | insert $row.key $row.value }
    } else {
        {}
    }
    cd $ROOT
    with-env ($variables | merge {PATH: ($dirs ++ $env.PATH)}) $command
}

# Runs `first`, then `after` whether or not `first` failed, then fails as `first`
# did. A fix that stops partway has still rewritten files, which `after` formats;
# a bump that stops partway has still moved versions, which `after` copies.
def then-always [first: closure, after: closure] {
    let failure = try { do $first; null } catch {|error| $error }
    do $after
    if $failure != null {
        error make {msg: $failure.msg}
    }
}

def main [] {
    ^nu $env.CURRENT_FILE --help
}

# Builds every shipped binary through Buck.
def "main build" [] {
    in-repo { ^buck2 build root//: }
}

# Builds every shipped binary through Buck in release mode.
def "main build-release" [] {
    in-repo { ^buck2 build -m release root//: }
}

# Runs the unit and integration tests through Buck.
#
# Doctests are the one part of `cargo test --workspace` this misses: the prelude
# runs them only with a nightly toolchain.
def "main test" [] {
    in-repo { ^buck2 test root//crates/... }
}

# Lints the first-party Rust graph through Buck (clippy and rustfmt).
def "main lint" [] {
    in-repo { ^nu scripts/lint.nu }
}

# Rewrites formatting with the rustfmt Buck pins.
#
# Over the files Buck owns, with the same binary, file list and arguments `lint`
# checks with.
def "main fmt" [] {
    in-repo { ^nu scripts/lint.nu rustfmt --write }
}

# Autofixes with Cargo, lints through Buck, then formats.
def "main fix-all" [] {
    then-always {
        in-repo {
            # Buck has no autofix, so this stays on Cargo. --allow-staged because
            # jj has no index of its own and anything in git's aborts the fix.
            ^cargo clippy --fix --all-features --all-targets --allow-dirty --allow-staged
            # The strict pass: `cargo clippy --fix` exits 0 even when lints it
            # cannot fix remain. Buck resolves features per target and lints the
            # test targets and wt-web as built for wasm32, which no single Cargo
            # invocation sees.
            ^nu scripts/lint.nu clippy
        }
    } { main fmt }
}

# Cargo: builds the workspace.
def "main cargo-build" [] {
    in-repo { ^cargo build --workspace }
}

# Cargo: type-checks all features and the WASM target.
def "main cargo-check" [] {
    in-repo {
        ^cargo check --all-features
        ^cargo check -p wt-web --target wasm32-unknown-unknown
    }
}

# Cargo: clippy over the whole workspace, warnings denied.
def "main cargo-lint" [] {
    in-repo { ^cargo clippy --workspace --all-targets --all-features -- -D warnings }
}

# Cargo: runs the complete test suite, including what Buck cannot.
def "main cargo-test" [] {
    # The zgui tests each open a window and a graphics device. One per core
    # wedges the run on a machine with many of them.
    in-repo { with-env {RUST_TEST_THREADS: "4"} { ^cargo test --workspace } }
}

# Cargo: applies rustc's own suggestions, then formats.
def "main cargo-fix" [] {
    then-always { in-repo { ^cargo fix --all-features --allow-dirty --allow-staged } } { main fmt }
}

# Cargo: applies clippy's machine-applicable suggestions, then formats.
def "main cargo-clippy-fix" [] {
    then-always { in-repo { ^cargo clippy --fix --all-features --all-targets --allow-dirty --allow-staged } } { main fmt }
}

# Bumps the published crates by `level` (major, minor or patch).
#
# Moves the workspace dependencies on them too, then copies the versions into
# Buck.
def "main bump" [level: string] {
    then-always {
        in-repo {
            for crate in $PUBLISHED {
                ^cargo set-version -p $crate --bump $level
            }
            ^cargo upgrade ...($PUBLISHED | each {|crate| [-p $crate] } | flatten) --incompatible allow --pinned allow
        }
    } { main sync-buck-versions }
}

# Copies Cargo package versions into Buck and the Windows resource metadata.
def "main sync-buck-versions" [] {
    in-repo { ^nu scripts/update_buck_versions.nu }
}

# Regenerates THIRD_PARTY_NOTICES.txt from the dependency graph.
def "main update-notices" [] {
    in-repo { ^nu scripts/update_third_party_notices.nu }
}

# Installs the reindeer that regenerates third-party/rust/BUCK.
#
# It is not on crates.io under that name, and another revision rewrites much of
# that file, so the revision is pinned. Its repository holds several packages
# with binaries, hence the package name.
def "main install-reindeer" [] {
    ^cargo install --locked --git https://github.com/facebookincubator/reindeer --rev 864a276e53a23b0509307f9069e75c7e0958c8c1 reindeer
}

# Installs the cargo-about that regenerates THIRD_PARTY_NOTICES.txt.
#
# scripts/update_third_party_notices.nu requires this version, the one
# flake.lock pins for the dev shell.
def "main install-cargo-about" [] {
    ^cargo install --locked cargo-about@0.8.4
}

# Downloads and locates the tools a native Cargo build needs.
#
# NASM, clang and llvm-ar. What it finds is written to .tooling/dev.env, which
# the cargo-* commands read.
def "main setup" [] {
    in-repo { ^python scripts/setup.py }
}
