#!/usr/bin/env python3
"""Generate the Rust per-version consumable id->name table from extracted JSON.

Reads the JSON produced by scripts/extract_consumable_ids.py, collapses runs of
consecutive versions with an identical id->name map into version-bracketed layout
entries, and emits a generated Rust source file for wowsunpack.

Usage: python scripts/gen_consumable_ids_rs.py <in.json> <out.rs>
"""
import json
import sys


def friendly(label):
    """Map a binary-dump label (`A.B.C`) to the real friendly version replays report.

    WoWs shipped as `0.X.Y` until the 12.0 rebrand, but the dump labels are
    inconsistent: some keep the leading `0.` (0.6.x, 0.7.0-0.7.6, 0.11.x) and some
    drop it (7.8.0 == 0.7.8, 9.10.0 == 0.9.10). `clientVersionFromExe` always reports
    the `0.X.Y` form pre-12.0, so we normalise to match. Verified build-monotonic
    across every available build.
    """
    a, b, c = (list(map(int, label.split("."))) + [0, 0, 0])[:3]
    if a == 0:
        return (0, b, c)  # already 0-prefixed
    if a < 12:
        return (0, a, b)  # 0-dropped legacy (third field is a hotfix, not the patch)
    return (a, b, c)  # modern (12.0+)


def main():
    in_path = sys.argv[1] if len(sys.argv) > 1 else ".scratch/consumable_ids.json"
    out_path = (
        sys.argv[2]
        if len(sys.argv) > 2
        else "crates/wowsunpack/src/consumable_versions.rs"
    )
    data = json.load(open(in_path, encoding="utf-8"))
    # Normalise dump labels to true friendly versions, ordered by (version, build).
    items = sorted(
        ((friendly(label), int(data[label]["build"]), data[label]["ids"]) for label in data),
        key=lambda t: (t[0], t[1]),
    )

    # Collapse consecutive builds sharing an identical id->name map into runs, keyed
    # on friendly version. Where one friendly version spans multiple hotfix builds
    # with differing layouts, the earliest build's layout represents it (replays only
    # carry major.minor.patch, so finer builds are indistinguishable at lookup time).
    runs = []  # (friendly_tuple, ids_dict)
    prev_sig = None
    emitted_versions = set()
    for fr, _build, ids in items:
        sig = json.dumps(ids, sort_keys=True)
        if sig != prev_sig and fr not in emitted_versions:
            runs.append((fr, ids))
            emitted_versions.add(fr)
            prev_sig = sig
        elif sig != prev_sig:
            prev_sig = sig  # layout changed within an already-emitted version; keep first

    lines = []
    lines.append(
        "//! Per-version consumable id -> name tables.\n"
        "//!\n"
        "//! Recovered by static analysis of the obfuscated consumable-constants module in\n"
        "//! each shipped client build (the `ConsumablesTypes` id ordering combined with\n"
        "//! `ConsumableNamesMap`). Keyed on friendly version (major.minor.patch): a replay\n"
        "//! resolves to the latest layout whose version it `is_at_least`. Regenerate with\n"
        "//! `scripts/extract_consumable_ids.py` then `scripts/gen_consumable_ids_rs.py`.\n"
        "//!\n"
        "//! @generated -- do not edit by hand.\n"
    )
    lines.append("use crate::data::Version;\n")
    lines.append("const fn v(major: u32, minor: u32, patch: u32) -> Version {")
    lines.append("    Version { major, minor, patch, build: 0 }")
    lines.append("}\n")
    lines.append(
        "/// Consumable id -> name layouts, ascending by version. Each table is the full\n"
        "/// id -> name map effective from that version until the next entry supersedes it."
    )
    lines.append("pub static CONSUMABLE_ID_LAYOUTS: &[(Version, &[(i32, &str)])] = &[")
    for (a, b, c), ids in runs:
        pairs = ", ".join(f'({i}, "{ids[i]}")' for i in sorted(ids, key=int))
        lines.append(f"    (v({a}, {b}, {c}), &[{pairs}]),")
    lines.append("];\n")
    lines.append(
        "/// The consumable id -> name table effective for `version`: the latest layout\n"
        "/// whose friendly version is <= `version`. Versions older than every known layout\n"
        "/// floor to the earliest one (the closest available approximation)."
    )
    lines.append(
        "pub fn consumable_ids_for_version(version: Version) -> Option<&'static [(i32, &'static str)]> {"
    )
    lines.append("    CONSUMABLE_ID_LAYOUTS")
    lines.append("        .iter()")
    lines.append("        .rev()")
    lines.append("        .find(|(start, _)| version.is_at_least(start))")
    lines.append("        .or_else(|| CONSUMABLE_ID_LAYOUTS.first())")
    lines.append("        .map(|(_, table)| *table)")
    lines.append("}")

    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print(f"wrote {out_path}: {len(runs)} layouts from {len(items)} builds")


if __name__ == "__main__":
    main()
