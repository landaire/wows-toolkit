# -*- coding: utf-8 -*-
"""Extract the pre-rework (<0.10) captain skill grid from old builds' GameParams.

Modern builds keep the grid in the client (see skill_grids_extract.py); old
builds carry tier+column per skill in GameParams.data, shared across ship
species. This unpickles each old build's GameParams.data (reverse + zlib +
generic unpickler), finds a captain's Skills dict, and records each skill's
(internal_name, tier, column). tier is stored 0-based (point cost - 1) to match
the modern table; column is kept as-is and only used for left-to-right ordering.

Run under Python 2.7:
  python2 scripts/skill_grids_extract_old.py <out.json>
"""
import io
import json
import os
import pickle
import sys
import zlib

BUILDS = r"G:/wows_builds"
MODERN_MIN_BUILD = 3343484  # >= this is handled by the client-table extractor


def unpickle_game_params(path):
    raw = zlib.decompress(open(path, "rb").read()[::-1])

    class G(object):
        def __setstate__(self, s):
            if isinstance(s, dict):
                self.__dict__.update(s)
            elif isinstance(s, tuple) and len(s) == 2 and isinstance(s[0], dict):
                self.__dict__.update(s[0])

    def factory(*a, **k):
        return G()

    class U(pickle.Unpickler):
        def find_class(self, m, n):
            return factory

    return U(io.BytesIO(raw)).load()


def loose_int(v):
    try:
        return int(v)
    except (TypeError, ValueError):
        try:
            return int(str(v).strip())
        except (TypeError, ValueError):
            return None


class _Found(Exception):
    def __init__(self, skills):
        self.skills = skills


def collect_skills(obj):
    """Find the first captain's Skills dict (a dict whose values carry
    column/tier/skillType) and return {internal_name: skill_dict}. Every crew
    shares the same skill set, so the first one suffices; stopping there keeps
    this from walking the whole multi-MB param graph."""
    seen = set()

    def walk(o, depth):
        if depth > 16 or id(o) in seen:
            return
        seen.add(id(o))
        # The skills container may be a plain dict or an object whose __dict__
        # maps skill name -> skill object.
        if isinstance(o, dict):
            container = o
        elif isinstance(o, (list, tuple)):
            for v in o:
                walk(v, depth + 1)
            return
        else:
            dd = getattr(o, "__dict__", None)
            container = dd if isinstance(dd, dict) else None
        if container is None:
            return
        hits = {}
        for k, v in container.items():
            vd = getattr(v, "__dict__", None)
            if isinstance(k, str) and isinstance(vd, dict) and "column" in vd and "tier" in vd and "skillType" in vd:
                hits[k] = vd
        if len(hits) >= 4:
            raise _Found(hits)
        for v in container.values():
            walk(v, depth + 1)

    try:
        walk(obj, 0)
    except _Found as f:
        return f.skills
    return {}


def main():
    out_path = sys.argv[1]
    dirs = []
    for d in sorted(os.listdir(BUILDS)):
        p = os.path.join(BUILDS, d)
        if not os.path.isdir(p):
            continue
        try:
            b = int(d.rsplit("_", 1)[1])
        except (IndexError, ValueError):
            continue
        if b >= MODERN_MIN_BUILD:
            continue
        if os.path.exists(os.path.join(p, "vfs", "content", "GameParams.data")):
            dirs.append((b, d))
    dirs.sort()

    results = {}
    for b, d in dirs:
        gp = os.path.join(BUILDS, d, "vfs", "content", "GameParams.data")
        try:
            top = unpickle_game_params(gp)
        except Exception as e:
            sys.stderr.write("[%s] unpickle failed: %s\n" % (d, e))
            continue
        skills = collect_skills(top)
        if not skills:
            sys.stderr.write("[%s] no skills dict found\n" % d)
            continue
        grid = []
        for name, dd in skills.items():
            tier = loose_int(dd.get("tier"))
            column = loose_int(dd.get("column"))
            if tier is None or column is None:
                continue
            grid.append({"skill": name, "tier": max(tier - 1, 0), "column": column})
        grid.sort(key=lambda s: (s["tier"], s["column"]))
        results[d] = {"build": b, "version": d.rsplit("_", 1)[0], "grid": grid}
        sys.stderr.write("[%s] ok: %d skills\n" % (d, len(grid)))

    with open(out_path, "wb") as f:
        f.write(json.dumps(results, indent=1, sort_keys=True).encode("utf-8"))
    sys.stderr.write("wrote %s (%d builds)\n" % (out_path, len(results)))


if __name__ == "__main__":
    main()
