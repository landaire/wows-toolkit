#!/usr/bin/env nu

let original_config = (open --raw .buckconfig.local)

def restore [config: string] {
    $config | save -f .buckconfig.local
    ^buck2 kill
}

# Outside the dev shell nothing rewrites a missing configuration, and Buck has to
# say what writes it.
rm .buckconfig.local
let result = (do {
    hide-env --ignore-errors WT_BUCK_TOOLCHAIN WT_BUCK_SYSTEM_LIBS
    ^buck2 kill
    ^buck2 audit execution-platform-resolution //:wgcheck
} | complete)
restore $original_config

if $result.exit_code == 0 {
    error make {msg: "Buck2 accepted a missing toolchain configuration."}
}

# Either half of the machine-local configuration may be reported first; both
# diagnostics have to name the bootstrap that writes it.
let diagnostic = $"($result.stdout)($result.stderr)"
if not ($diagnostic | str contains "refresh-buck-toolchain.nu") {
    error make {msg: $"Missing-toolchain diagnostic does not name the bootstrap: ($diagnostic)"}
}
if not (($diagnostic | str contains "Missing [nix_toolchain] root") or ($diagnostic | str contains "Missing [hermetic_tools]")) {
    error make {msg: $"Unexpected missing-toolchain diagnostic: ($diagnostic)"}
}

# Inside the dev shell bin/buck2 replaces a configuration that names another
# toolchain with the one the shell was evaluated with.
let toolchain = ($env.WT_BUCK_TOOLCHAIN? | default "")
if ($toolchain | is-not-empty) {
    $original_config | str replace $"root = ($toolchain)" "root = /nix/store/stale-wows-toolkit-buck-toolchain" | save -f .buckconfig.local
    let refreshed = (do { ^buck2 --version } | complete)
    let config = (open --raw .buckconfig.local)
    restore $original_config

    if $refreshed.exit_code != 0 {
        error make {msg: $"bin/buck2 failed with a stale toolchain configuration: ($refreshed.stderr)"}
    }
    if not ($config | lines | any {|line| $line == $"root = ($toolchain)" }) {
        error make {msg: "bin/buck2 left a configuration naming another toolchain in place."}
    }
}
