#!/usr/bin/env nu

# Every third-party crate Buck links into a distributed desktop binary is listed
# in THIRD_PARTY_NOTICES.txt. The generator resolves each package through Cargo;
# Buck resolves third-party features once for the whole workspace, which can add
# crates no single package pulls in under Cargo.

def main [] {
    # The Windows platform configures only on a provisioned Windows host, and the
    # Nix ones only elsewhere.
    let platforms = if $nu.os-info.name == "windows" {
        ["toolchains//platforms:windows_x86_64_msvc"]
    } else {
        ["toolchains//platforms:linux_x86_64" "toolchains//platforms:macos_arm64"]
    }

    let tools = (open build-support/release-tools.json | get tools.target)
    let targets = (["//:wows_toolkit_zgui" "//:wows_toolkit_egui"] | append $tools | str join " ")
    let query = $"kind\('rust_library', deps\(set\(($targets)\)\)\)"

    # Each crate of an entry's "Used by:" list is on its own indented line.
    let listed = (open --raw THIRD_PARTY_NOTICES.txt | lines | where {|line| $line starts-with "    " } | str trim | uniq)
    let first_party = (open third-party/licenses/notices.toml | get first_party.name)

    let missing = ($platforms | each {|platform|
        let result = (do { ^buck2 cquery --target-platforms $platform $query --output-attribute "^(env|crate_root)$" --json } | complete)
        if $result.exit_code != 0 {
            error make {msg: $"buck2 cquery failed for ($platform):\n($result.stderr)"}
        }

        $result.stdout
        | from json
        | transpose label attributes
        | where {|target| $target.label | str starts-with "root//third-party/rust:" }
        | each {|target| crate-identity $target }
        | where {|crate| not ($crate in $listed) and not (($crate | split row " " | first) in $first_party) }
        | each {|crate| {platform: $platform, crate: $crate} }
    } | flatten)

    if not ($missing | is-empty) {
        print ($missing | sort-by crate)
        error make {msg: "THIRD_PARTY_NOTICES.txt does not list every crate Buck links. Add the package or feature that pulls them in to a root in third-party/licenses/notices.toml and run `nu scripts/update_third_party_notices.nu`."}
    }

    print "THIRD_PARTY_NOTICES.txt lists every third-party crate of the distributed binaries."
}

# "name version" as the notices list it. Reindeer omits the package name and
# version from the environment of some registry crates; their crate_root then
# starts with the "<name>-<version>.crate" directory.
def crate-identity [target: record] {
    let variables = ($target.attributes.env? | default {})
    if ($variables.CARGO_PKG_NAME? != null) and ($variables.CARGO_PKG_VERSION? != null) {
        return $"($variables.CARGO_PKG_NAME) ($variables.CARGO_PKG_VERSION)"
    }

    let archive = ($target.attributes.crate_root | path split | first)
    let parsed = ($archive | parse --regex '^(?<name>.+)-(?<version>\d+\.\d+\.\d+.*)\.crate$')
    if ($parsed | is-empty) {
        error make {msg: $"Cannot tell the package and version of ($target.label)."}
    }

    $"($parsed.0.name) ($parsed.0.version)"
}
