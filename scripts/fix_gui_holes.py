#!/usr/bin/env python3
"""Fill gui-asset holes in archived dumps from the nearest build that has them.

The asset audit found a handful of builds missing gui categories (modernization
icons, achievements, nation flags, crew skills, signal flags, ribbons) that both
older and newer builds have -- partial dumps (the delta build of a download pair,
or a span where a gui package wasn't fetched), not version-dependent absences.

These categories are stable across nearby versions, and the toolkit reads each
dump's vfs/ tree directly, so we copy the missing category dir from the nearest
build (by version) that has it. This is a content fill, not a fresh extraction;
for a metadata-exact re-dump use `wows-data-mgr complete-build --with-gui`.
"""
import os
import re
import shutil

ARCH = r"G:\wows_builds"
# audit name -> path under vfs/
CATEGORIES = {
    "modernization_icons": "gui/modernization_icons",
    "achievements": "gui/achievements",
    "nation_flags": "gui/nation_flags",
    "crew_skills": "gui/crew_commander/skills",
    "signal_flags": "gui/signal_flags",
    "ribbons": "gui/ribbons",
}


def vkey(name):
    base = name.rsplit("_", 1)[0]
    a, b, c = (list(map(int, base.split("."))) + [0, 0, 0])[:3]
    return (0, a, b) if 0 < a < 12 else ((0, b, c) if a == 0 else (a, b, c))


def count(build_dir, rel):
    d = os.path.join(ARCH, build_dir, "vfs", rel)
    return len(os.listdir(d)) if os.path.isdir(d) else 0


def main():
    builds = sorted(
        (d for d in os.listdir(ARCH) if re.match(r"^\d", d) and os.path.isdir(os.path.join(ARCH, d))),
        key=vkey,
    )
    total = 0
    for rel in CATEGORIES.values():
        have = [(i, b) for i, b in enumerate(builds) if count(b, rel) > 0]
        if not have:
            continue
        first, last = have[0][0], have[-1][0]
        have_idx = {i for i, _ in have}
        for i in range(first, last + 1):
            if i in have_idx:
                continue
            # A hole: pick the nearest build (by index) that has this category.
            src_i = min(have_idx, key=lambda j: abs(j - i))
            src = os.path.join(ARCH, builds[src_i], "vfs", rel)
            dst = os.path.join(ARCH, builds[i], "vfs", rel)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            if os.path.isdir(dst):
                shutil.rmtree(dst)
            shutil.copytree(src, dst)
            n = len(os.listdir(dst))
            print(f"FILL {builds[i]:18} {rel:28} <- {builds[src_i]} ({n} files)")
            total += 1
    print(f"\nfilled {total} category holes")


if __name__ == "__main__":
    main()
