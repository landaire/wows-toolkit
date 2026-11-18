#!/usr/bin/env nu

# Regenerates THIRD_PARTY_NOTICES.txt from the dependency graph of every package
# whose build is distributed, plus the components third-party/licenses/notices.toml
# lists by hand.
#
# Features are resolved for the whole workspace at once, per target, as reindeer
# resolves them for third-party/rust/BUCK. A crate that another workspace
# member's features pull into a distributed binary is covered as it is under
# Buck. cargo-about supplies the resolution and the license texts.
#
# Needs cargo-about at CARGO_ABOUT_VERSION: `nix develop` provides the one
# flake.lock pins, and `nu scripts/dev.nu install-cargo-about` installs the same one
# elsewhere. Needs the network unless every crate source is already fetched.
#
# `--check` fails when the committed file differs from what this generates.

const CARGO_ABOUT_VERSION = "0.8.4"
const CONFIG_DIR = "third-party/licenses"
const OUTPUT = "THIRD_PARTY_NOTICES.txt"
# The Rust parser in crates/wows-toolkit-notices divides the file at this line.
const SEPARATOR = "--------------------------------------------------------------------------------"
# The copyright lines of the SPDX MIT and BSD templates, and of a GitHub MIT template.
const PLACEHOLDER_COPYRIGHT = '<year>\s+<owner>|<copyright holders>|\[year\]\s+\[fullname\]'
# The license a crate offered under it is distributed under when its preferred one is a
# placeholder template. Its text names no copyright holder.
const APACHE_TEXT = "third-party/licenses/texts/apache-2.0.txt"

def main [--check] {
    let version = (^cargo-about --version | str trim)
    if $version != $"cargo-about ($CARGO_ABOUT_VERSION)" {
        error make {msg: $"Expected cargo-about ($CARGO_ABOUT_VERSION), found '($version)'. Run inside `nix develop`, or `nu scripts/dev.nu install-cargo-about`."}
    }

    let config = (open ($CONFIG_DIR | path join "notices.toml"))
    let default_targets = (open ($CONFIG_DIR | path join "about.toml") | get targets)

    # Roots resolved for the same targets share one cargo-about run.
    let root_sets = ($config.root
        | each {|root| {manifest: ($root.manifest | path expand), targets: ($root.targets? | default $default_targets)} }
        | group-by {|root| $root.targets | str join " " }
        | values
        | each {|roots| {targets: ($roots | first | get targets), manifests: ($roots | get manifest)} })

    let reports = ($root_sets | each {|set|
        let report = (run-cargo-about $set.targets)
        let ids = (distributed-packages $set.manifests $set.targets ($report.crates | get package.id))
        {
            crates: ($report.crates | where {|entry| $entry.package.id in $ids })
            licenses: ($report.licenses
                | each {|license| $license | update used_by ($license.used_by | where {|usage| $usage.crate.id in $ids }) }
                | where {|license| not ($license.used_by | is-empty) })
        }
    })

    let crates = ($reports
        | each {|report| $report.crates }
        | flatten
        | where {|entry| is-third-party $entry.package }
        | insert package_id {|entry| $entry.package.id }
        | uniq-by package_id)

    let unresolved = ($crates | where {|entry| ($entry.license == "Unknown") or ($entry.license | str contains "NOASSERTION") })
    if not ($unresolved | is-empty) {
        let names = ($unresolved | each {|entry| $"($entry.package.name) ($entry.package.version)" } | str join ", ")
        error make {msg: $"cargo-about found no license for: ($names). Add a clarification to about.toml."}
    }

    let resolved = ($crates | get package)
    let unresolved_crates = (unresolved-crates ($config.unresolved_crate? | default []) $resolved)
    let packages = ($resolved | append ($unresolved_crates | each {|entry| $entry.package }))
    let license_groups = (crate-license-groups $reports
        | append ($unresolved_crates | each {|entry| $entry.group })
        | with-apache-for-templates ($crates | select package_id license)
        | with-crate-licenses ($config.crate_license? | default []) $packages)

    # The SPDX templates cargo-about falls back to; their terms require the real notice.
    let templated = ($license_groups | where {|group| $group.text =~ $PLACEHOLDER_COPYRIGHT })
    if not ($templated | is-empty) {
        let names = ($templated | each {|group| $group.crates | each {|crate| $"($crate.name) ($crate.version)" } } | flatten | str join ", ")
        error make {msg: $"License text with placeholder copyright for: ($names). Add a clarification with the real copyright line to about.toml, or a crate_license to notices.toml."}
    }

    let covered = ($license_groups | each {|group| $group.crates | get id } | flatten | uniq)
    let uncovered = ($packages | where {|package| not ($package.id in $covered) })
    if not ($uncovered | is-empty) {
        let names = ($uncovered | each {|package| $"($package.name) ($package.version)" } | str join ", ")
        error make {msg: $"cargo-about produced no license text for: ($names)."}
    }

    let entries = (
        ($config.component | each {|component| component-entry $component $packages })
        | append ($license_groups | each {|group| license-entry $group })
        | append (notice-entries $packages)
        | append (mpl-entry $license_groups $config.component)
    ) | unique-titles

    let text = (render $entries)

    if $check {
        let committed = (read-text $OUTPUT)
        if $committed != $text {
            error make {msg: $"($OUTPUT) is out of date. Run `nu scripts/update_third_party_notices.nu`."}
        }
        print $"($OUTPUT) is up to date."
        return
    }

    $text | save --force $OUTPUT
    print $"Wrote ($OUTPUT): ($packages | length) crates, ($config.component | length) other components, ($entries | length) entries."
    print "Crates per license; a crate under several licenses counts under each:"
    $license_groups
    | each {|group| $group.crates | each {|crate| {license: $group.id, crate: $crate.id} } }
    | flatten
    | uniq
    | group-by license
    | items {|license, rows| {license: $license, crates: ($rows | length)} }
    | sort-by crates --reverse
    | print
}

# A crate that packages no license file gets the SPDX template, whose copyright
# line is a placeholder. One also offered under Apache-2.0 alone is distributed
# under that instead. Any other is left for the placeholder check.
def with-apache-for-templates [licenses: list] {
    let groups = $in
    let moved = ($groups
        | where {|group| $group.text =~ $PLACEHOLDER_COPYRIGHT }
        | get crates
        | flatten
        | where {|crate|
            let declared = ($licenses | where package_id == $crate.id)
            not ($declared | is-empty) and (offers-apache ($declared | first | get license))
        }
        | uniq-by id
        | sort-by name version)
    if ($moved | is-empty) {
        return $groups
    }

    let ids = ($moved | get id)
    $groups
    | each {|group| $group | update crates ($group.crates | where {|crate| not ($crate.id in $ids) }) }
    | where {|group| not ($group.crates | is-empty) }
    | append {id: "Apache-2.0", name: "Apache License 2.0", text: (normalize (read-text $APACHE_TEXT)), crates: $moved}
}

# Whether `expression` offers Apache-2.0 as an alternative on its own. Old
# manifests separate alternatives with "/".
def offers-apache [expression: string] {
    if ($expression | str contains " AND ") {
        return false
    }
    "Apache-2.0" in ($expression | str replace --all "/" " OR " | str replace --all --regex '[()]' "" | split row " OR " | str trim)
}

# Replaces the texts cargo-about gives each crate listed under crate_license in
# notices.toml with the files it names: `crate_files` from the crate's source,
# then `files` from this repository. Crates given the same text share an entry.
def with-crate-licenses [entries: list, packages: list] {
    let groups = $in
    let overrides = ($entries
        | each {|entry|
            let matches = ($packages | where {|package| $package.name == $entry.name and $package.version == $entry.version })
            if ($matches | is-empty) {
                error make {msg: $"($entry.name) ($entry.version) is not in the dependency graph. Update crate_license in notices.toml."}
            }
            let package = ($matches | first)
            let dir = ($package.manifest_path | path dirname)
            let paths = (($entry.crate_files? | default [] | each {|file| $dir | path join $file })
                | append ($entry.files? | default []))
            if ($paths | is-empty) {
                error make {msg: $"crate_license for ($entry.name) ($entry.version) names no file."}
            }
            let text = ($paths | each {|path| normalize (read-text $path) } | str join "\n\n")
            {
                key: ($"($entry.license)\n($text)" | hash sha256)
                id: $entry.license
                name: $entry.license
                text: $text
                crate: ($package | select id name version source)
            }
        }
        | group-by key
        | values
        | each {|rows|
            let first = ($rows | first)
            {id: $first.id, name: $first.name, text: $first.text, crates: ($rows | get crate | sort-by name version)}
        }
        | sort-by {|group| $"($group.name)\n($group.crates | first | get name)" })
    let ids = ($overrides | each {|group| $group.crates | get id } | flatten)

    $groups
    | each {|group| $group | update crates ($group.crates | where {|crate| not ($crate.id in $ids) }) }
    | where {|group| not ($group.crates | is-empty) }
    | append $overrides
}

# The front ends key their notice lists by title. Two texts of one license whose
# first crate is the same would otherwise share one.
def unique-titles [] {
    let entries = $in
    $entries | enumerate | each {|row|
        let earlier = ($entries | first $row.index | where title == $row.item.title | length)
        if $earlier == 0 { $row.item } else { $row.item | update title $"($row.item.title) \(text ($earlier + 1)\)" }
    }
}

# Crates listed under unresolved_crate in notices.toml: Buck links them, but
# cargo-about's resolution leaves them out. Their license files are read from
# the crate source.
def unresolved-crates [entries: list, resolved: list] {
    if ($entries | is-empty) {
        return []
    }

    let packages = (^cargo metadata --format-version 1 --locked | from json | get packages)
    $entries | each {|entry|
        if ($resolved | any {|package| $package.name == $entry.name }) {
            error make {msg: $"cargo-about resolves ($entry.name) now. Remove it from unresolved_crate in notices.toml."}
        }

        let matches = ($packages | where name == $entry.name)
        if ($matches | length) != 1 {
            error make {msg: $"Expected one version of ($entry.name) in Cargo.lock, found ($matches | length)."}
        }

        let package = ($matches | first)
        let dir = ($package.manifest_path | path dirname)
        {
            package: $package
            group: {
                id: $entry.license
                name: $entry.license
                text: ($entry.files | each {|file| normalize (read-text ($dir | path join $file)) } | str join "\n\n")
                crates: [($package | select id name version source)]
            }
        }
    }
}

# The packages reachable from `manifests` through normal and build dependencies,
# for any of `targets`. The lock file's graph also holds optional dependencies no
# feature enables; a registry or git package counts only when it is in `enabled`,
# the graph cargo-about resolved with the workspace's features.
def distributed-packages [manifests: list, targets: list, enabled: list] {
    $targets | each {|target|
        print $"cargo metadata: ($target)"
        let metadata = (^cargo metadata --format-version 1 --locked --filter-platform $target | from json)

        let roots = ($metadata.packages | where {|package| ($package.manifest_path | path expand) in $manifests } | get id)
        if ($roots | length) != ($manifests | length) {
            error make {msg: $"Not every root in notices.toml is a workspace member: ($manifests | str join ', ')"}
        }

        let members = ($metadata.packages | where source == null | get id)
        let edges = ($metadata.resolve.nodes | each {|node| {
            id: $node.id
            deps: ($node.deps
                | where {|dep| $dep.dep_kinds | any {|kind| $kind.kind != "dev" } }
                | get pkg
                | where {|id| ($id in $enabled) or ($id in $members) })
        } })

        mut reached = $roots
        mut frontier = $roots
        while not ($frontier | is-empty) {
            let current = $frontier
            let known = $reached
            $frontier = ($edges | where {|node| $node.id in $current } | get deps | flatten | uniq | where {|id| not ($id in $known) })
            $reached = ($reached | append $frontier)
        }

        $reached
    } | flatten | uniq
}

def run-cargo-about [targets: list] {
    let target_args = ($targets | each {|target| ["--target" $target] } | flatten)
    # Half the machine: cargo-about scans every license file of every crate on
    # all cores by default.
    let threads = ([1 ((sys cpu | length) // 2)] | math max)

    print $"cargo-about: ($targets | str join ', ')"
    let result = (with-env {RAYON_NUM_THREADS: ($threads | into string)} {
        do {
            (^cargo-about generate --format json --locked --workspace
                --config ($CONFIG_DIR | path join "about.toml")
                ...$target_args)
        } | complete
    })
    if $result.exit_code != 0 {
        error make {msg: $"cargo-about failed for ($targets | str join ', '):\n($result.stderr)"}
    }

    $result.stdout | from json
}

# Workspace members have no source.
def is-third-party [package: record] {
    $package.source != null
}

# One group per distinct license text, each with the crates distributed under it.
def crate-license-groups [reports: list] {
    $reports
    | each {|report| $report.licenses }
    | flatten
    | each {|license|
        let text = (normalize $license.text)
        $license.used_by
        | where {|usage| is-third-party $usage.crate }
        | each {|usage| {
            key: ($"($license.id)\n($text)" | hash sha256)
            id: $license.id
            name: $license.name
            text: $text
            crate: ($usage.crate | select id name version source)
        } }
    }
    | flatten
    | group-by key
    | values
    | each {|rows|
        let first = ($rows | first)
        {
            id: $first.id
            name: $first.name
            text: $first.text
            crates: ($rows | get crate | uniq-by id | sort-by name version)
        }
    }
    | insert sort_key {|group| $"($group.name)\n($group.crates | first | get name)\n($group.crates | first | get version)" }
    | sort-by sort_key
    | reject sort_key
}

def license-entry [group: record] {
    {
        title: $"($group.name): (summary ($group.crates))"
        body: ([(used-by $group.crates) "" $group.text] | str join "\n")
    }
}

def component-entry [component: record, packages: list] {
    let container = if ($component.contained_in? != null) {
        let versions = ($packages | where name == $component.contained_in | sort-by version | get version)
        if ($versions | is-empty) {
            error make {msg: $"($component.name): ($component.contained_in) is not in the dependency graph. Update notices.toml."}
        }
        $"Contained in: ($versions | each {|version| $'($component.contained_in) ($version)' } | str join ', ')"
    } else {
        $"Used in: ($component.used_in)"
    }

    let texts = ($component.files | each {|file|
        let base = if ($file.crate? != null) {
            let matches = ($packages | where name == $file.crate)
            if ($matches | is-empty) {
                error make {msg: $"($component.name): ($file.crate) is not in the dependency graph. Update notices.toml."}
            }
            $matches | sort-by version | last | get manifest_path | path dirname
        } else {
            "."
        }
        normalize (read-text ($base | path join $file.path))
    })

    let version = if ($component.version? != null) { $" ($component.version)" } else { "" }
    let offered = if ($component.offered? != null) { [$"Offered under: ($component.offered)"] } else { [] }
    let header = ([$"License: ($component.license)"] | append $offered | append [$"Source: ($component.url)" $container])

    {
        title: $"($component.name)($version) \(($component.license)\)"
        body: ($header | append "" | append ($texts | str join "\n\n") | str join "\n")
    }
}

# Apache-2.0 section 4(d) requires a NOTICE file to be passed on, and rav1e's
# PATENTS grant is a condition of its use. A git dependency can keep these at
# the repository root, above the crate.
def notice-entries [packages: list] {
    $packages
    | each {|package|
        notice-dirs $package
        | each {|dir| ls $dir | where type == file | get name }
        | flatten
        | where {|path| ($path | path basename) =~ '(?i)^(notice|patents)(\.(txt|md))?$' }
        | each {|path|
            let text = (normalize (read-text $path))
            {
                key: ($text | hash sha256)
                kind: ($path | path parse | get stem | str upcase)
                text: $text
                crate: ($package | select id name version source)
            }
        }
    }
    | flatten
    | group-by key
    | values
    | each {|rows|
        let first = ($rows | first)
        let crates = ($rows | get crate | uniq-by id | sort-by name version)
        {
            title: $"($first.kind) file: (summary $crates)"
            body: ([(used-by $crates) "" $first.text] | str join "\n")
        }
    }
    | sort-by title
}

def notice-dirs [package: record] {
    let crate_dir = ($package.manifest_path | path dirname)
    if not ($package.source | str starts-with "git+") {
        return [$crate_dir]
    }

    # Cargo checks a git dependency out at checkouts/<repository>/<revision>.
    let parts = ($crate_dir | path split)
    let checkouts = ($parts | enumerate | where item == "checkouts" | get index | last)
    let root_depth = $checkouts + 3
    ($root_depth..(($parts | length))) | each {|depth| $parts | first $depth | path join }
}

# MPL-2.0 section 3.2 requires telling recipients where the source of the
# covered files can be obtained.
def mpl-entry [groups: list, components: list] {
    let crates = ($groups | where id == "MPL-2.0" | get crates | flatten | uniq-by id | sort-by name version)
    let crate_lines = ($crates | each {|crate| $"    ($crate.name) ($crate.version): (source-location $crate)" })
    let component_lines = ($components
        | where {|component| $component.license | str contains "MPL-2.0" }
        | each {|component| $"    ($component.name): ($component.url)" })

    {
        title: "Source code of the MPL-2.0 components"
        body: ([
            "The components below are distributed under the Mozilla Public License 2.0. Their source"
            "code, including any file that WoWs Toolkit distributes in executable form, is available at"
            "the location given for each. The license text is under \"Mozilla Public License 2.0\" above."
            ""
        ] | append $crate_lines | append $component_lines | str join "\n")
    }
}

def source-location [crate: record] {
    if ($crate.source | str starts-with "registry+") {
        return $"https://crates.io/crates/($crate.name)/($crate.version)"
    }

    let location = ($crate.source | str replace "git+" "" | split row "#")
    let repository = ($location | first | split row "?" | first)
    $"($repository) at commit ($location | last)"
}

def summary [crates: list] {
    let first = ($crates | first)
    match ($crates | length) {
        1 => $"($first.name) ($first.version)"
        2 => $"($first.name) ($first.version), ($crates | get 1 | get name) ($crates | get 1 | get version)"
        $count => $"($first.name) ($first.version) and ($count - 1) other crates"
    }
}

def used-by [crates: list] {
    ["Used by:"] | append ($crates | each {|crate| $"    ($crate.name) ($crate.version)" }) | str join "\n"
}

def render [entries: list] {
    let python = if (which python3 | is-not-empty) { "python3" } else { "python" }
    let digest = (^$python scripts/check-third-party-notices.py --digest | str trim)

    let preamble = [
        "WoWs Toolkit third-party notices"
        ""
        "WoWs Toolkit contains software written by third parties. Its desktop applications, command-line"
        "tools, installers and web clients distribute the components listed in this file. Each entry gives"
        "a license, the components distributed under it, and the license text with the copyright notices"
        "the authors supply. Where a component is offered under a choice of licenses, the entry gives the"
        "license WoWs Toolkit distributes it under."
        ""
        "WoWs Toolkit itself is distributed under the MIT License. See LICENSE."
        ""
        "This file is generated by scripts/update_third_party_notices.nu."
        $"Input digest: sha256:($digest)"
    ]

    let sections = ($entries | each {|entry|
        if ([$entry.title $entry.body] | any {|text| $text | lines | any {|line| $line == $SEPARATOR } }) {
            error make {msg: $"The entry '($entry.title)' contains the separator line."}
        }
        [$SEPARATOR $entry.title $SEPARATOR "" $entry.body ""] | str join "\n"
    })

    ($preamble | str join "\n") + "\n\n" + ($sections | str join "\n")
}

def read-text [path: string] {
    let raw = (open --raw $path)
    if ($raw | describe) == "binary" { $raw | decode utf-8 } else { $raw }
}

def normalize [text: string] {
    $text | str replace --all "\r\n" "\n" | lines | each {|line| $line | str trim --right } | str join "\n" | str trim --char "\n"
}
