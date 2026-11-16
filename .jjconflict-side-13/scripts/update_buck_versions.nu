#!/usr/bin/env nu

def workspace-version [] {
    open Cargo.toml | get workspace.package.version
}

def crate-version [dir: path] {
    let declared = (open ($dir | path join "Cargo.toml") | get package.version)
    if ($declared | describe) == "string" { $declared } else { workspace-version }
}

def replace-prefixed-lines [path: path prefix: string replacement: string] {
    let contents = (open --raw $path)
    let newline = if ($contents | str contains "\r\n") { "\r\n" } else { "\n" }
    let has_trailing_newline = ($contents | str ends-with $newline)
    let lines = ($contents | lines)
    let matches = ($lines | where {|line| $line | str starts-with $prefix })
    if ($matches | is-empty) {
        error make {msg: $"($path) has no line beginning with '($prefix)'"}
    }
    let updated = ($lines | each {|line|
        if ($line | str starts-with $prefix) { $replacement } else { $line }
    })
    let updated = if $has_trailing_newline { $updated | append "" } else { $updated }
    $updated | str join $newline | save --force $path
    $matches | length
}

mut changed_files = 0
for buckfile in (glob "crates/*/BUCK") {
    let dir = ($buckfile | path dirname)
    let version = (crate-version $dir)
    let count = (replace-prefixed-lines $buckfile "    version = " $'    version = "($version)",')
    print $"($buckfile): set ($count) target versions to ($version)"
    $changed_files = $changed_files + 1
}

let release_version = (workspace-version)
let msi_version = ($release_version | split row "-" | first)
let msi_entries = (replace-prefixed-lines "toolchains/windows/BUCK" "    version = " $'    version = "($msi_version)",')
let rc_numeric = ($msi_version | split row "." | append "0" | str join ",")
let rc_file_entries = (replace-prefixed-lines "assets/wows_toolkit.rc" "FILEVERSION " $"FILEVERSION ($rc_numeric)")
let rc_product_entries = (replace-prefixed-lines "assets/wows_toolkit.rc" "PRODUCTVERSION " $"PRODUCTVERSION ($rc_numeric)")
let rc_file_string_entries = (replace-prefixed-lines "assets/wows_toolkit.rc" '            VALUE "FileVersion", ' $'            VALUE "FileVersion", "($release_version)"')
let rc_product_string_entries = (replace-prefixed-lines "assets/wows_toolkit.rc" '            VALUE "ProductVersion", ' $'            VALUE "ProductVersion", "($release_version)"')
let rc_total = ($rc_file_entries + $rc_product_entries + $rc_file_string_entries + $rc_product_string_entries)
print $"toolchains/windows/BUCK: set ($msi_entries) MSI version entries to ($msi_version)"
print $"assets/wows_toolkit.rc: synchronized ($rc_total) version entries"

^nu scripts/test-workspace-package-metadata.nu
if $env.LAST_EXIT_CODE != 0 {
    exit $env.LAST_EXIT_CODE
}
