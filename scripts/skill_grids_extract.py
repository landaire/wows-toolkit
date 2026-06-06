# -*- coding: utf-8 -*-
"""Extract the captain skill grid (SKILLS_BY_SHIP_TYPE) from each modern WoWS
build's CommonCrewSkills module.

For each build dir under wows_binaries that ships scripts/CommonCrewSkills.pyc:
  1. extract the .pyc to a work dir (never touching wows_binaries),
  2. deobfuscate+decompile with wowsdeob (--final-only),
  3. eval the SKILLS_BY_SHIP_TYPE literal with stub namespaces so ST.* ->
     skill name, SKILL_GROUP.* -> group rank, ShipTypes.* -> type name,
  4. derive per-ship-type (tier, column, group) for every skill, matching the
     client's getSkillColumn ordering (sort groups by rank, chain, 1-based col).

Run under Python 2.7. Usage:
  python2 extract.py <out.json> [build_dir ...]   # default: all builds with the module
"""
import json
import os
import re
import subprocess
import sys
import zipfile

BINARIES = r"G:/wows_binaries"
WORK = r"G:/dev/skillgrid_extract/work"
WOWSDEOB = r"G:/dev/wowsdeob/target/release/wowsdeob.exe"
MODULE = "scripts/CommonCrewSkills.pyc"

# SKILL_GROUP ranks from shared_constants (ATTACK=0, DEFENCE=1, SUPPORT=2,
# ACUMEN=3); column order sorts groups by this rank.
GROUP_RANK = {"ATTACK": 0, "DEFENCE": 1, "SUPPORT": 2, "ACUMEN": 3}
RANK_NAME = {v: k for k, v in GROUP_RANK.items()}


class NameNs(object):
    """Attribute access returns the attribute name as a string (ST, ShipTypes)."""

    def __getattr__(self, k):
        return k


class GroupNs(object):
    """Attribute access returns the group's sort rank (matches the client)."""

    def __getattr__(self, k):
        return GROUP_RANK[k]


def build_id(dirname):
    return int(dirname.rsplit("_", 1)[1])


def version_str(dirname):
    return dirname.rsplit("_", 1)[0]


def have_module(zip_path):
    try:
        return MODULE in set(zipfile.ZipFile(zip_path).namelist())
    except Exception:
        return False


def deob_module(dirname):
    """Return decompiled CommonCrewSkills.py text for a build, or None."""
    src_zip = os.path.join(BINARIES, dirname, "scripts.zip")
    work = os.path.join(WORK, dirname)
    out = os.path.join(work, "out")
    if not os.path.isdir(out):
        os.makedirs(out)
    pyc = os.path.join(work, "CommonCrewSkills.pyc")
    with zipfile.ZipFile(src_zip) as z:
        with open(pyc, "wb") as f:
            f.write(z.read(MODULE))
    try:
        subprocess.check_output(
            [WOWSDEOB, "--final-only", pyc, out], stderr=subprocess.STDOUT
        )
    except subprocess.CalledProcessError as e:
        sys.stderr.write("[%s] wowsdeob failed: %s\n" % (dirname, e.output[-200:]))
        return None
    py = os.path.join(out, "CommonCrewSkills.py")
    if not os.path.isfile(py):
        sys.stderr.write("[%s] no decompiled .py produced\n" % dirname)
        return None
    return open(py, "rb").read().decode("utf-8", "replace")


def parse_table(text):
    """Eval the SKILLS_BY_SHIP_TYPE literal -> {shipType: (tierdict, ...)}."""
    m = re.search(r"^SKILLS_BY_SHIP_TYPE\s*=\s*(\{.*\})\s*$", text, re.M)
    if not m:
        return None
    g = {"ST": NameNs(), "ShipTypes": NameNs(), "SKILL_GROUP": GroupNs()}
    return eval(m.group(1), g)


def grid_for_build(table):
    """{shipType: [ {skill, tier, column, group}, ... ]} from the eval'd table."""
    out = {}
    for ship_type, tiers in table.items():
        rows = []
        for tier_idx, tier_dict in enumerate(tiers):
            col = 0
            for rank in sorted(tier_dict.keys()):
                for skill in tier_dict[rank]:
                    col += 1
                    rows.append(
                        {
                            "skill": skill,
                            "tier": tier_idx,
                            "column": col,
                            "group": RANK_NAME.get(rank, str(rank)),
                        }
                    )
        out[ship_type] = rows
    return out


def main():
    out_path = sys.argv[1]
    if len(sys.argv) > 2:
        dirs = sys.argv[2:]
    else:
        dirs = [
            d
            for d in sorted(os.listdir(BINARIES))
            if os.path.isfile(os.path.join(BINARIES, d, "scripts.zip"))
            and have_module(os.path.join(BINARIES, d, "scripts.zip"))
        ]
        dirs.sort(key=build_id)

    results = {}
    for d in dirs:
        text = deob_module(d)
        if text is None:
            continue
        table = parse_table(text)
        if table is None:
            sys.stderr.write("[%s] SKILLS_BY_SHIP_TYPE not found\n" % d)
            continue
        grid = grid_for_build(table)
        results[d] = {
            "build": build_id(d),
            "version": version_str(d),
            "grid": grid,
        }
        nship = len(grid)
        nsk = sum(len(v) for v in grid.values())
        sys.stderr.write("[%s] ok: %d ship types, %d skill slots\n" % (d, nship, nsk))

    with open(out_path, "wb") as f:
        f.write(json.dumps(results, indent=1, sort_keys=True).encode("utf-8"))
    sys.stderr.write("wrote %s (%d builds)\n" % (out_path, len(results)))


if __name__ == "__main__":
    main()
