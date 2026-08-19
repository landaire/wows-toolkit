#!/usr/bin/env nu

# Buck files carry a `version =` string per target because the Rust rules have
# no way to read Cargo.toml. Nothing else notices when the two drift, and a
# stale one compiles the wrong CARGO_PKG_VERSION into the shipped binary.

def workspace-version [] {
    open Cargo.toml | get workspace.package.version
}

def crate-version [dir: path] {
    let declared = (open ($dir | path join "Cargo.toml") | get package.version)
    if ($declared | describe) == "string" { $declared } else { workspace-version }
}

# A crate on an edition other than the workspace one has to repeat it in its
# BUCK file. Getting this wrong compiles the crate under the wrong language
# edition, which changes semantics, not just lints.
def crate-edition [dir: path] {
    let declared = (open ($dir | path join "Cargo.toml") | get package.edition)
    if ($declared | describe) == "string" { $declared } else { open Cargo.toml | get workspace.package.edition }
}

def buck-editions [buckfile: path] {
    open --raw $buckfile
    | lines
    | where {|line| $line | str starts-with "    edition = " }
    | each {|line| $line | str replace --all --regex '^\s+edition = "(.*)",$' '$1' }
}

def buck-versions [buckfile: path] {
    open --raw $buckfile
    | lines
    | where {|line| $line | str starts-with "    version = " }
    | each {|line| $line | str replace --all --regex '^\s+version = "(.*)",$' '$1' }
}

def main [] {
    mut failures = []

    for dir in (glob crates/*/BUCK | each {|p| $p | path dirname }) {
        let expected = (crate-version $dir)
        for found in (buck-versions ($dir | path join "BUCK")) {
            if $found != $expected {
                $failures = ($failures | append $"($dir)/BUCK declares version ($found), Cargo.toml says ($expected)")
            }
        }

        let expected_edition = (crate-edition $dir)
        let buckfile = ($dir | path join "BUCK")
        let found_editions = (buck-editions $buckfile)
        for found in $found_editions {
            if $found != $expected_edition {
                $failures = ($failures | append $"($dir)/BUCK declares edition ($found), Cargo.toml says ($expected_edition)")
            }
        }
        # An omitted edition takes workspace_rules.bzl's default, so silence
        # here is only correct when the crate is on the workspace edition.
        let workspace_edition = (open Cargo.toml | get workspace.package.edition)
        if ($found_editions | is-empty) and $expected_edition != $workspace_edition {
            $failures = ($failures | append $"($dir)/BUCK omits edition, but Cargo.toml pins ($expected_edition)")
        }
    }

    # An MSI ProductVersion field must be numeric, so the installer ships under
    # the release version even when the tag carries a pre-release suffix.
    let msi_expected = (workspace-version | split row "-" | first)
    let msi_found = (buck-versions toolchains/windows/BUCK)
    if $msi_found != [$msi_expected] {
        $failures = ($failures | append $"toolchains/windows/BUCK declares MSI version ($msi_found), expected ($msi_expected)")
    }

    # The resource script replaces what winresource derived from Cargo.toml
    # under Cargo, so the exe's version resource is hand-maintained here.
    let rc = (open --raw assets/wows_toolkit.rc)
    let rc_numeric = ($msi_expected | split row "." | append "0" | str join ",")
    for field in ["FILEVERSION" "PRODUCTVERSION"] {
        if not ($rc | str contains $"($field) ($rc_numeric)") {
            $failures = ($failures | append $"assets/wows_toolkit.rc is missing ($field) ($rc_numeric)")
        }
    }
    for field in ["FileVersion" "ProductVersion"] {
        if not ($rc | str contains $'VALUE "($field)", "(workspace-version)"') {
            $failures = ($failures | append $'assets/wows_toolkit.rc ($field) is not (workspace-version)')
        }
    }

    if not ($failures | is-empty) {
        error make {msg: $"Buck package metadata does not match Cargo.toml:\n($failures | str join "\n")"}
    }
}
