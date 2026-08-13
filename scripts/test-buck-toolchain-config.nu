#!/usr/bin/env nu

let original_config = (open --raw .buckconfig.local)

try {
    rm .buckconfig.local
    ^buck2 kill
    let result = (do { ^buck2 audit execution-platform-resolution //:wgcheck } | complete)
    if $result.exit_code == 0 {
        error make "Buck2 accepted a missing nix_toolchain root."
    }

    let diagnostic = $"($result.stdout)($result.stderr)"
    if not ($diagnostic | str contains "Missing [nix_toolchain] root") {
        error make $"Unexpected missing-toolchain diagnostic: ($diagnostic)"
    }
} finally {
    $original_config | save -f .buckconfig.local
    ^buck2 kill
}
