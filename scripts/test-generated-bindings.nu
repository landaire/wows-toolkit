#!/usr/bin/env nu

# Some crates' build scripts generate bindings with bindgen, which needs a host
# SDK and libclang that a hermetic action cannot reach, so the output is checked
# in beside the fixup and OUT_DIR points at it. Nothing in the build notices
# when the crate moves on: the stale bindings still compile as long as the
# symbols the crate imports have not changed, and the ones that did change are
# then wrong. Each such fixup records the version it was generated from.

# Fixup directory -> the crate whose build script it stands in for.
const PINNED = [
    [fixup, crate];
    ["third-party/rust/fixups/gpui-pre-media", "gpui-pre-media"],
]

def locked-version [crate: string] {
    open --raw Cargo.lock
    | from toml
    | get package
    | where name == $crate
    | get version
}

def main [] {
    mut failures = []

    for entry in $PINNED {
        let pin_file = ($entry.fixup | path join "generated-for-version")
        if not ($pin_file | path exists) {
            $failures = ($failures | append $"($pin_file) is missing")
            continue
        }

        let pinned = (open --raw $pin_file | str trim)
        let locked = (locked-version $entry.crate)

        if ($locked | is-empty) {
            $failures = ($failures | append $"($entry.crate) is not in Cargo.lock, so ($entry.fixup) is dead")
            continue
        }
        if ($locked | length) > 1 {
            $failures = ($failures | append $"($entry.crate) is in Cargo.lock ($locked | length) times: ($locked | str join ', ')")
            continue
        }

        let locked = ($locked | first)
        if $pinned != $locked {
            $failures = ($failures | append $"($entry.crate) is ($locked) but ($entry.fixup) holds bindings generated from ($pinned). Regenerate them on a host with the SDK, per ($entry.fixup)/BUCK, and update ($pin_file).")
        }
    }

    if not ($failures | is-empty) {
        error make {msg: $"Checked-in generated bindings are stale:\n($failures | str join "\n")"}
    }
}
