#!/usr/bin/env nu

let original_config = (open --raw .buckconfig.local)

def restore [config: string] {
    $config | save -f .buckconfig.local
    ^buck2 kill
}

rm .buckconfig.local
^buck2 kill
let result = (do { ^buck2 audit execution-platform-resolution //:wgcheck } | complete)
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
