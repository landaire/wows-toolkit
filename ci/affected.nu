#!/usr/bin/env nu

# Selects the first-party targets a change affects, so CI checks only those.
#
# A target is affected when its recursive target hash differs from the one at
# --base: the hash covers the target's attributes, its source contents, and the
# hashes of everything it depends on, third-party crates and toolchains
# included. Third-party targets themselves are never selected; they are built
# only as dependencies of a first-party target.
#
# A target hash does not cover rule implementations, Buck configuration, or the
# checks' own scripts, so a change to any of those selects everything.
#
# Writes JSON to --out:
#   full         every check runs
#   aliases      root aliases to build
#   tests        test targets to run
#   lint         a query expression of the targets to lint, or null
#   graph_tests  whether scripts/test-*.nu run

const FIRST_PARTY = ["root//:" "root//crates/..."]

# Paths whose change no target hash reflects.
const FULL_PREFIXES = [
    ".buckconfig"
    ".github/workflows/buck.yml"
    ".rustfmt.toml"
    "bin/"
    "bootstrap."
    "build-support/"
    "bxl/"
    "ci/"
    "clippy.toml"
    "flake."
    "platforms/"
    "prelude/"
    "reindeer.toml"
    "rust-toolchain.toml"
    "scripts/"
    # Shared fixtures, which tests read at run time without declaring.
    "tests/"
    "toolchains/"
]
const FULL_FILES = ["Cargo.toml" "PACKAGE"]
const FULL_SUFFIXES = [".bzl" ".bxl" "/PACKAGE"]

# The graph tests read these directly, so no target hash covers them.
const GRAPH_TEST_SUFFIXES = [
    "BUCK"
    "Cargo.lock"
    "Cargo.toml"
    "build.rs"
    "fixups.toml"
    "generated-for-version"
]
const GRAPH_TEST_PREFIXES = ["assets/"]

def full_result [reason: string] {
    {
        full: true
        reason: $reason
        aliases: ["root//:"]
        tests: ["root//crates/..." "root//ci/..."]
        lint: "root//..."
        graph_tests: true
    }
}

def needs_full [path: string] {
    (
        ($path in $FULL_FILES)
        or ($FULL_PREFIXES | any {|prefix| $path | str starts-with $prefix })
        or ($FULL_SUFFIXES | any {|suffix| $path | str ends-with $suffix })
    )
}

# Label to recursive hash and rule type, for every first-party target in the
# checkout at `root`.
def target_hashes [root: string] {
    let result = (do {
        cd $root
        (^buck2 targets ...$FIRST_PARTY --json --show-target-hash
            --target-hash-recursive=true --target-hash-file-mode=paths_and_contents
            --output-attribute "^(name|buck\\.(type|target_hash|package))$")
    } | complete)
    if $result.exit_code != 0 {
        return null
    }
    $result.stdout | from json | each {|t|
        {
            label: $"($t."buck.package"):($t.name)"
            hash: $t."buck.target_hash"
            type: ($t."buck.type" | split row ":" | last)
        }
    }
}

def select_affected [base: string] {
    if ($base | is-empty) {
        return (full_result "no successful mainline run to compare against")
    }
    let present = (do { ^git cat-file -e $"($base)^{commit}" } | complete)
    if $present.exit_code != 0 {
        let fetched = (do { ^git fetch --no-tags --depth=1 origin $base } | complete)
        if $fetched.exit_code != 0 {
            return (full_result $"could not fetch ($base)")
        }
    }

    # Compares trees, so it holds whether or not the base is an ancestor.
    let changed = (^git diff --name-only $base | lines)
    let forcing = ($changed | where {|path| needs_full $path })
    if not ($forcing | is-empty) {
        return (full_result $"($forcing | first) changed")
    }

    let worktree = (mktemp -d -t wt-affected-base.XXXXXX)
    ^git worktree add --detach --force $worktree $base o+e>| ignore
    # The base parses under this checkout's toolchain. Any toolchain change is a
    # flake.* or toolchains/ change, which selected everything above.
    cp .buckconfig.local $worktree
    let before = try { target_hashes $worktree } catch { null }
    do { cd $worktree; ^buck2 kill } | complete | ignore
    ^git worktree remove --force $worktree
    if $before == null {
        return (full_result $"the graph at ($base) does not parse")
    }
    let after = (target_hashes $env.PWD)
    if $after == null {
        error make {msg: "buck2 targets failed on this checkout."}
    }

    let old = ($before | select label hash | rename label old)
    let affected = ($after | join --left $old label | where {|t| $t.old? != $t.hash })

    let aliases = ($affected | where {|t| $t.label | str starts-with "root//:" } | get label)
    let crates = ($affected | where {|t| $t.label | str starts-with "root//crates/" })
    # A test can read files from its crate at run time that it never declares,
    # such as fixtures, so any change in a crate runs all of that crate's tests.
    let touched = ($changed | where {|path| $path | str starts-with "crates/" } | each {|path|
        $"root//crates/($path | split row '/' | get 1):"
    } | uniq)
    let crate_tests = ($after | where {|t| ($t.type | str ends-with "_test") and ($touched | any {|p| $t.label | str starts-with $p }) })
    let tests = ($crates | where {|t| $t.type | str ends-with "_test" } | append $crate_tests | get label | uniq)
    let lint = if ($crates | is-empty) { null } else { $"set\(($crates.label | str join ' ')\)" }
    {
        full: false
        reason: $"($affected | length) first-party target\(s\) changed since ($base)"
        aliases: $aliases
        # Seconds to run, and they read manifests no target hash covers.
        tests: ($tests | append "root//ci/...")
        lint: $lint
        graph_tests: ($changed | any {|path| (
            ($GRAPH_TEST_SUFFIXES | any {|suffix| $path | str ends-with $suffix })
            or ($GRAPH_TEST_PREFIXES | any {|prefix| $path | str starts-with $prefix })
        ) })
    }
}

def main [
    --base: string = ""  # commit of the last successful mainline run; empty selects everything
    --full               # select everything regardless of --base
    --out: string        # where to write the JSON selection
] {
    let selection = if $full { full_result "a full run was requested" } else { select_affected $base }
    print $"($selection.reason): ($selection.aliases | length) alias\(es\), ($selection.tests | length) test target\(s\)"
    $selection | to json | save -f $out
}
