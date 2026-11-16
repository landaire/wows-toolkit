#!/usr/bin/env nu

def main [] {
    let data_scripts = [
        "crates/wowsunpack/build.rs"
        "crates/wows-replays/build.rs"
        "crates/minimap-renderer/build.rs"
        "crates/wows-battle-world/build.rs"
    ]

    for script in $data_scripts {
        let source = (open $script)
        let root = ($source | split row "fn game_data_source" | get 1?)
        if $root == null or not ($root | str starts-with "()") {
            error make {msg: $"($script) does not select its game data source in a dedicated helper."}
        }
        if not ($root | str contains 'WOWS_GAME_DATA') or not ($root | str contains 'WOWS_HERMETIC_BUILD') {
            error make {msg: $"($script) does not recognize the declared hermetic game data input."}
        }
        let hermetic_scan = ($source | split row "fn discover_builds" | get 1? | default "")
        if not ($hermetic_scan | str contains 'scan_data_directories') {
            error make {msg: $"($script) may scan undeclared game data during a hermetic build."}
        }
        if not ($source | str contains 'cargo:rerun-if-env-changed=WOWS_HERMETIC_BUILD') {
            error make {msg: $"($script) does not rerun when its hermetic data mode changes."}
        }
    }

    for fixup in [
        "third-party/rust/fixups/wowsunpack/fixups.toml"
        "third-party/rust/fixups/wows_replays/fixups.toml"
        "third-party/rust/fixups/wows_minimap_renderer/fixups.toml"
        "third-party/rust/fixups/wows-battle-world/fixups.toml"
    ] {
        let contents = (open --raw $fixup)
        if not ($contents | str contains '[buildscript.build]') or not ($contents | str contains 'extra_deps = ["//build-support/no-game-data:versions_toml"]') {
            error make {msg: $"($fixup) does not declare the hermetic registry input."}
        }
        if not ($contents | str contains 'WOWS_GAME_DATA = "$(location //build-support/no-game-data:versions_toml)"') {
            error make {msg: $"($fixup) does not pass the declared registry path."}
        }
        if not ($contents | str contains 'WOWS_HERMETIC_BUILD = "1"') {
            error make {msg: $"($fixup) does not suppress undeclared game-data scans."}
        }
    }

    let toolkit_build = (open --raw "crates/wows-toolkit/build.rs")
    if not ($toolkit_build | str contains 'EMBEDDED_CONSTANTS') or not ($toolkit_build | str contains 'WOWS_TOOLKIT_ICON') or not ($toolkit_build | str contains 'OUT_DIR') {
        error make {msg: "The toolkit build script does not stage its declared embedded resources."}
    }

    let toolkit_fixup = (open --raw "third-party/rust/fixups/wows_toolkit/fixups.toml")
    if ($toolkit_fixup | str trim) != "buildscript.run = false" {
        error make {msg: "The wows-toolkit build script must be disabled for native Buck actions."}
    }
}
