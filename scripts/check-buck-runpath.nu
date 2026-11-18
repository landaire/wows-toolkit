#!/usr/bin/env nu

# A built ELF file may name only store paths and paths relative to itself in its
# runpath. Any other directory came from the machine that built it, such as a
# checkout path from a shell's linker flags or a host library directory, and a
# cached action hands that file to every other machine.
const ALLOWED = '^(/nix/store/|\$ORIGIN)'

# The runpath and rpath entries of `file`, or none when it is not an ELF file.
def runpaths [file: path] {
    let dynamic = (do { ^readelf -d $file } | complete)
    if $dynamic.exit_code != 0 {
        return []
    }
    $dynamic.stdout
    | lines
    | parse --regex '\((?:RUNPATH|RPATH)\)\s+Library r(?:un)?path: \[(?<paths>[^\]]*)\]'
    | get paths
    | each {|paths| $paths | split row ':' }
    | flatten
}

def main [target: string, native_build_mode = "debug"] {
    if $native_build_mode not-in ["debug", "release"] {
        error make {msg: $"Unsupported native build mode: ($native_build_mode)"}
    }
    if $nu.os-info.name != "linux" {
        return
    }

    let mode = if $native_build_mode == "release" { ["-m" "release"] } else { [] }
    let outputs = (
        ^buck2 build ...$mode $target --show-full-output
        | lines
        | each {|line| $line | split row ' ' | skip 1 | str join ' ' }
    )
    let files = (
        $outputs
        | each {|output|
            if ($output | path type) == "dir" {
                glob $"($output)/**/*" | where {|file| ($file | path type) == "file" }
            } else {
                [$output]
            }
        }
        | flatten
    )

    let foreign = (
        $files
        | each {|file| runpaths $file | where {|entry| $entry !~ $ALLOWED } | each {|entry| {file: $file, entry: $entry} } }
        | flatten
    )
    if ($foreign | is-not-empty) {
        print -e ($foreign | first 20 | table)
        error make {msg: $"($target) built a file whose runpath names a directory outside the store."}
    }
}
