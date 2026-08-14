#!/usr/bin/env nu

const PROBE = "//tests/buck-hermetic:opt_level_probe[rustc_flags]"

def probe_rustc_flags [config: list<string>] {
    let probe = (^buck2 build --show-output ...$config $PROBE | complete)
    if $probe.exit_code != 0 {
        error make {msg: $"Buck build failed while checking build-script mode propagation: ($probe.stderr)"}
    }
    let output = ($probe.stdout | lines | where $it =~ 'rustc_flags' | last | split row " " | last)
    open --raw $output | str trim
}

def main [] {
    let debug_target = (^buck2 cquery //:wows_toolkit | complete)
    let release_target = (^buck2 cquery -c native_build.mode=release //:wows_toolkit | complete)

    if $debug_target.exit_code != 0 or $release_target.exit_code != 0 {
        error make {msg: "Buck cquery failed while checking native build modes."}
    }
    if $debug_target.stdout == $release_target.stdout {
        error make {msg: "Debug and release use the same configured target identity."}
    }

    let debug_actions = (^buck2 aquery 'all_actions(deps(//:wows_toolkit))' --output-all-attributes | complete)
    let release_actions = (^buck2 aquery -c native_build.mode=release 'all_actions(deps(//:wows_toolkit))' --output-all-attributes | complete)

    if $debug_actions.exit_code != 0 or $release_actions.exit_code != 0 {
        error make {msg: "Buck aquery failed while checking native build modes."}
    }
    if not ($debug_actions.stdout | str contains "-Copt-level=0") {
        error make {msg: "Debug action graph is missing -Copt-level=0."}
    }
    if not ($release_actions.stdout | str contains "-Copt-level=3") {
        error make {msg: "Release action graph is missing -Copt-level=3."}
    }
    # The probe is not behind the build-mode transition, so both modes write the
    # same output path. Read each result before building the other.
    let debug_flags = (probe_rustc_flags [])
    let release_flags = (probe_rustc_flags ["-c" "native_build.mode=release"])

    if $debug_flags != "--env-set=BUILD_SCRIPT_OPT_LEVEL=0" {
        error make {msg: "Debug build scripts are missing OPT_LEVEL=0 propagation."}
    }
    if $release_flags != "--env-set=BUILD_SCRIPT_OPT_LEVEL=3" {
        error make {msg: "Release build scripts are missing OPT_LEVEL=3 propagation."}
    }
    let hard_coded_buildscript_profile = (
        open --raw third-party/rust/BUCK
        | lines
        | where $it =~ '"(PROFILE|OPT_LEVEL|DEBUG)": "'
    )
    if ($hard_coded_buildscript_profile | is-not-empty) {
        error make {msg: "A third-party build script hard-codes the build profile."}
    }
}
