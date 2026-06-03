import os, re, glob
ARCH = r"G:\wows_builds"
# category -> (relative glob under vfs/, or special)
CATS = {
    "maps":            ("spaces/*/minimap.png", None),
    "GameParams":      ("content/GameParams.data", None),
    "entities.xml":    ("scripts/entities.xml", None),
    "entity_defs":     ("scripts/entity_defs/*", None),
    "translations":    (None, "translations/en/LC_MESSAGES/global.mo"),  # outside vfs/
    "consumables":     ("gui/consumables/*", None),
    "modernization":   ("gui/modernization_icons/*", None),
    "ship_icons(svg)": ("gui/fla/minimap/ship_icons/*.svg", None),
    "fonts(ttf)":      ("gui/fonts/*.ttf", None),
    "fonts(bitmap)":   ("gui/fonts/*/*.png", None),
    "ribbons":         ("gui/ribbons/*", None),
    "achievements":    ("gui/achievements/*", None),
    "nation_flags":    ("gui/nation_flags/*", None),
    "crew_skills":     ("gui/crew_commander/skills/*", None),
    "signal_flags":    ("gui/signal_flags/*", None),
    "silhouettes":     ("gui/ships_silhouettes/*", None),
    "battle_hud":      ("gui/battle_hud/*", None),
}

def vkey(name):
    base = name.rsplit("_", 1)[0]
    a, b, c = (list(map(int, base.split("."))) + [0, 0, 0])[:3]
    return (0, a, b) if 0 < a < 12 else ((0, b, c) if a == 0 else (a, b, c))

builds = sorted([d for d in os.listdir(ARCH) if re.match(r"^\d", d) and os.path.isdir(os.path.join(ARCH, d))],
                key=vkey)

counts = {}  # cat -> [(build, n)]
for d in builds:
    root = os.path.join(ARCH, d)
    vfs = os.path.join(root, "vfs")
    for cat, (g, special) in CATS.items():
        if special:
            n = 1 if os.path.exists(os.path.join(root, special)) else 0
        else:
            n = len(glob.glob(os.path.join(vfs, g)))
        counts.setdefault(cat, []).append((d, n))

# Hole detection: a build with 0 where an EARLIER and a LATER build both have >0.
print(f"{'category':16} intro_at           holes (missing despite older+newer present)")
for cat, series in counts.items():
    present = [i for i, (_, n) in enumerate(series) if n > 0]
    if not present:
        print(f"{cat:16} NEVER PRESENT")
        continue
    first, last = present[0], present[-1]
    intro = series[first][0] if first > 0 else "(from start)"
    holes = [series[i][0] for i in range(first, last + 1) if series[i][1] == 0]
    tag = "" if not holes else f"  <-- {len(holes)} HOLES"
    print(f"{cat:16} {intro:18} {('none' if not holes else ', '.join(holes[:8]))}{tag}")
