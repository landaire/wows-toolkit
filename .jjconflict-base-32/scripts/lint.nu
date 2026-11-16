#!/usr/bin/env nu

# Lints the first-party Rust graph through Buck.
#
# The prelude builds clippy diagnostics with infallible_diagnostics = True, so
# the clippy action succeeds even when compilation fails: BUILD SUCCEEDED says
# nothing about whether the code lints. This reads the diagnostics themselves
# and sets the exit code from them.
#
# That covers a target whose own code does not compile. A dependency failing to
# compile is different: the clippy sub-target cannot be produced at all, so the
# BXL aborts and the buck2 error surfaces instead of a rendered diagnostic.
#
# rustfmt also writes, with --write: same pinned binary, same file list, same
# arguments as the check, so formatting cannot drift from what the gate wants.
# Clippy has no write mode here -- the rust rules produce diagnostics, not
# rewrites -- so `cargo clippy --fix` remains the only autofix (see mise.toml).
#
# Usage:
#   nu scripts/lint.nu              # clippy and rustfmt over root//...
#   nu scripts/lint.nu clippy
#   nu scripts/lint.nu rustfmt --target root//crates/wowsunpack/...
#   nu scripts/lint.nu rustfmt --write

# .rustfmt.toml keeps these commented out, so they are named here and nowhere
# else: the check and the write below are the only two callers. The rustfmt binary
# comes from toolchains//:rustfmt via the BXL, not from PATH.
const RUSTFMT_ARGS = [
    "--config"
    "unstable_features=true"
    "--config"
    "imports_granularity=Item"
    "--config"
    "imports_layout=Vertical"
]

def run_bxl [entry: string, target: string] {
    let result = (do { ^buck2 bxl $"//bxl/lint.bxl:($entry)" -- --target $target } | complete)
    if $result.exit_code != 0 {
        print -e $result.stderr
        error make {msg: $"buck2 bxl //bxl/lint.bxl:($entry) failed."}
    }
    $result.stdout | from json
}

def check_clippy [target: string] {
    let report = (run_bxl "clippy" $target)
    # A pattern that matches nothing must not read as a pass: a typo, a rename,
    # or a filter that widens would otherwise turn this into a permanent green.
    if ($report.diagnostics | is-empty) {
        error make {msg: $"bxl/lint.bxl:clippy matched no targets under ($target)."}
    }

    mut findings = []
    for entry in ($report.diagnostics | transpose target paths) {
        for path in $entry.paths {
            if not ($path | path exists) {
                error make {msg: $"Missing clippy diagnostics for ($entry.target): ($path)"}
            }
            # One JSON object per line; an empty file means the target is clean.
            let diagnostics = (
                open --raw $path
                | lines
                | where {|line| ($line | str trim) != "" }
                | each {|line| $line | from json }
            )
            for diagnostic in $diagnostics {
                let level = ($diagnostic | get -o level | default "")
                let message = ($diagnostic | get -o message | default "")
                # rustc closes with span-less tallies ("2 warnings emitted",
                # "aborting due to 1 previous error"). Counting those would
                # double-report every finding.
                let is_summary = (
                    ($message =~ '^\d+ (warning|error)s? emitted$')
                    or ($message =~ '^aborting due to')
                )
                if $level in ["error" "warning"] and not $is_summary {
                    $findings = ($findings | append {target: $entry.target, level: $level})
                    print -e ($diagnostic | get -o rendered | default "")
                }
            }
        }
    }

    let target_count = ($report.diagnostics | columns | length)
    if ($findings | is-empty) {
        print $"clippy: clean across ($target_count) target\(s\)."
        return true
    }

    let errors = ($findings | where level == "error" | length)
    let warnings = ($findings | where level == "warning" | length)
    print -e $"clippy: ($errors) error\(s\) and ($warnings) warning\(s\) across ($findings | get target | uniq | length) of ($target_count) target\(s\)."
    false
}

def check_rustfmt [target: string, write: bool] {
    let report = (run_bxl "rustfmt" $target)
    let groups = ($report.sources_by_edition | transpose edition sources)
    if ($groups | is-empty) {
        error make {msg: $"bxl/lint.bxl:rustfmt matched no sources under ($target)."}
    }

    mut ok = true
    mut checked = 0
    for group in $groups {
        # Paths are repository-relative, so anchor them; the task must work from
        # a subdirectory.
        let paths = ($group.sources | each {|src| [$report.root $src] | path join })
        # Windows caps a command line at 32767 characters and the repository is
        # already past half that in one invocation.
        for chunk in ($paths | chunks 100) {
            # rustfmt parses to the edition it is told; a 2021 crate checked as
            # 2024 reports differences that are not real.
            # --check reports differences and writes nothing; without it rustfmt
            # rewrites the file in place. Nothing else differs between the two.
            let mode = (if $write { [] } else { ["--check"] })
            let result = (
                do { ^$report.rustfmt ...$mode --edition $group.edition ...$RUSTFMT_ARGS ...$chunk } | complete
            )
            if $result.exit_code != 0 {
                print -e $result.stdout
                print -e $result.stderr
                $ok = false
            }
        }
        $checked = $checked + ($group.sources | length)
    }

    if not $ok {
        if $write {
            print -e $"rustfmt: ($checked) file\(s\) could not all be formatted."
        } else {
            print -e $"rustfmt: formatting differences found \(($checked) file\(s\) checked\)."
        }
        return false
    }
    if $write {
        print $"rustfmt: ($checked) file\(s\) formatted."
    } else {
        print $"rustfmt: ($checked) file\(s\) already formatted."
    }
    true
}

def main [
    what: string = "all"  # clippy, rustfmt, or all
    --target: string = "root//..."
    --write  # rustfmt only: rewrite the files instead of reporting differences
] {
    if $write and $what == "clippy" {
        error make {msg: "clippy has no write mode here; use `cargo clippy --fix`."}
    }
    let checks = match $what {
        "all" => ["clippy" "rustfmt"]
        "clippy" => ["clippy"]
        "rustfmt" => ["rustfmt"]
        _ => { error make {msg: $"Unknown check ($what); expected clippy, rustfmt, or all."} }
    }

    mut ok = true
    for check in $checks {
        let passed = if $check == "clippy" { check_clippy $target } else { check_rustfmt $target $write }
        if not $passed { $ok = false }
    }
    if not $ok { exit 1 }
}
