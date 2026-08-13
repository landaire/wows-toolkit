#!/usr/bin/env nu

def main [] {
    let debug_target = (^buck2 cquery //:wows_toolkit | complete)
    let release_target = (^buck2 cquery -c native_build.mode=release //:wows_toolkit | complete)

    if $debug_target.exit_code != 0 or $release_target.exit_code != 0 {
        error make "Buck cquery failed while checking native build modes."
    }
    if $debug_target.stdout == $release_target.stdout {
        error make "Debug and release use the same configured target identity."
    }

    let debug_actions = (^buck2 aquery 'all_actions(deps(//:wows_toolkit))' --output-all-attributes | complete)
    let release_actions = (^buck2 aquery -c native_build.mode=release 'all_actions(deps(//:wows_toolkit))' --output-all-attributes | complete)

    if $debug_actions.exit_code != 0 or $release_actions.exit_code != 0 {
        error make "Buck aquery failed while checking native build modes."
    }
    if not ($debug_actions.stdout | str contains "-Copt-level=0") {
        error make "Debug action graph is missing -Copt-level=0."
    }
    if not ($release_actions.stdout | str contains "-Copt-level=3") {
        error make "Release action graph is missing -Copt-level=3."
    }
    let hard_coded_buildscript_profile = (^rg -n '"PROFILE": "debug"' third-party/rust/BUCK | complete)
    if $hard_coded_buildscript_profile.exit_code == 0 {
        error make "A third-party build script hard-codes the debug profile."
    }
}
