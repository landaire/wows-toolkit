#!/usr/bin/env nu

# Selects the first-party targets a change affects, so CI checks only those.
#
# A target is affected when its recursive target hash differs from the one at
# --base: the hash covers the target's attributes, its source contents, and the
# hashes of everything it depends on, third-party crates and toolchains
# included. Third-party targets themselves are never selected; they are built
# only as dependencies of a first-party target.
#
# A target hash does not cover rule implementations or Buck configuration, so a
# change to either selects everything. The checks are targets too: a change to
# the scripts of a check that runs only over affected targets selects
# everything. A change to the files the flake builds the toolchain from selects
# everything only when the toolchain store paths change.
#
# Writes JSON to --out:
#   full         every check runs
#   aliases      root aliases to build
#   tests        test targets to run
#   lint         a query expression of the targets to lint, or null
#   graph_tests  whether scripts/test-*.nu run

const FIRST_PARTY = ["root//:" "root//crates/..."]
const CHECKS = "root//ci/..."

# Paths whose change no target hash reflects.
const FULL_PREFIXES = [
    ".buckconfig"
    ".github/workflows/buck.yml"
    ".rustfmt.toml"
    "bin/"
    "bootstrap."
    "build-support/"
    "bxl/"
    "clippy.toml"
    "platforms/"
    "prelude/"
    "reindeer.toml"
    # Shared fixtures, which tests read at run time without declaring.
    "tests/"
    "toolchains/"
]
# The root Cargo.toml reaches Buck only through the generated BUCK files, which
# the target hashes cover.
const FULL_FILES = [
    "PACKAGE"
    # Writes .buckconfig.local, which names the toolchain.
    "scripts/refresh-buck-toolchain.nu"
    # Prepare the runners the release and Windows builds run on.
    "scripts/expand-runner-swap.sh"
    "scripts/prepare-windows-runner.ps1"
]
const FULL_SUFFIXES = [".bzl" ".bxl" "/PACKAGE"]

# What the flake builds the Buck toolchain and the CI shell from.
const TOOLCHAIN_INPUTS = ["flake.nix" "flake.lock" "rust-toolchain.toml"]
# The flake outputs CI uses, for each host CI builds on: the packages
# .buckconfig.local names, and the shell that runs buck2 and the nu checks.
const TOOLCHAIN_PATHS = '
    flake: let
      pick = system: [
        flake.packages.${system}.buck-toolchain.outPath
        flake.packages.${system}.buck-system-libs.outPath
        (flake.packages.${system}.macos-sdk.outPath or null)
        flake.devShells.${system}.default.outPath
      ];
    in { x86_64-linux = pick "x86_64-linux"; aarch64-darwin = pick "aarch64-darwin"; }
'

# The graph tests read these directly, so no target hash covers them.
const GRAPH_TEST_SUFFIXES = [
    "BUCK"
    "Cargo.lock"
    "Cargo.toml"
    "build.rs"
    "fixups.toml"
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

# Label to recursive hash and rule type, for every first-party target and check
# in the checkout at `root`.
def target_hashes [root: string] {
    let result = (do {
        cd $root
        (^buck2 targets ...$FIRST_PARTY $CHECKS --json --show-target-hash
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

# The toolchain store paths the flake at `rev` gives, or null when it does not
# evaluate. An empty `rev` reads the working tree. Only TOOLCHAIN_INPUTS are
# copied, so a flake that reads any other file does not evaluate here.
def toolchain_paths [rev: string] {
    let dir = (mktemp -d -t wt-affected-flake.XXXXXX)
    for file in $TOOLCHAIN_INPUTS {
        let content = try {
            if ($rev | is-empty) { open --raw $file } else { ^git show $"($rev):($file)" }
        } catch {
            null
        }
        if $content == null {
            rm -rf $dir
            return null
        }
        $content | save -f ($dir | path join $file)
    }
    # getFlake of a path that is not locked needs --impure.
    let expr = $"\(($TOOLCHAIN_PATHS)) \(builtins.getFlake \"path:($dir)\")"
    let result = (do { ^nix eval --impure --json --expr $expr } | complete)
    rm -rf $dir
    if $result.exit_code != 0 {
        return null
    }
    $result.stdout | from json
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
    # Evaluated on every run rather than only when TOOLCHAIN_INPUTS change, so a
    # flake that starts reading another file keeps selecting everything until
    # TOOLCHAIN_INPUTS lists it.
    let toolchain_after = (toolchain_paths "")
    if $toolchain_after == null {
        return (full_result "the flake does not evaluate from TOOLCHAIN_INPUTS alone")
    }
    if (toolchain_paths $base) != $toolchain_after {
        return (full_result "the toolchain or CI shell the flake gives changed")
    }

    let worktree = (mktemp -d -t wt-affected-base.XXXXXX)
    ^git worktree add --detach --force $worktree $base o+e>| ignore
    # The base parses under this checkout's toolchain. Any toolchain change
    # selected everything above.
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

    # A check that runs only over the affected targets runs over all of them once
    # its own scripts change. The script tests run in full on every run.
    let checks = ($affected | where {|t| $t.label | str starts-with "root//ci:" })
    let widened = ($checks | where {|t| $t.type == "nu_script" and $t.label != "root//ci:graph_tests" })
    if not ($widened | is-empty) {
        return (full_result $"($widened | first | get label) changed")
    }

    let aliases = ($affected | where {|t| ($t.label | str starts-with "root//:") and $t.type == "alias" } | get label)
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
        reason: $"($affected | length) target\(s\) changed since ($base)"
        aliases: $aliases
        # Seconds to run, and they read manifests no target hash covers.
        tests: ($tests | append "root//ci/...")
        lint: $lint
        graph_tests: (
            ($checks | any {|t| $t.label == "root//ci:graph_tests" })
            or ($changed | any {|path| (
                ($GRAPH_TEST_SUFFIXES | any {|suffix| $path | str ends-with $suffix })
                or ($GRAPH_TEST_PREFIXES | any {|prefix| $path | str starts-with $prefix })
            ) })
        )
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
