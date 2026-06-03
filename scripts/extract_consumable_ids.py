#!/usr/bin/env python3
"""Extract per-version consumable id->name maps from obfuscated WoWs scripts.

For each version dir under WOWS_BINARIES (named `<friendly>_<build>`), this locates
the consumable-constants module, deobfuscates it with wowsdeob, and parses the
`ConsumablesTypes` enum (id ordering) together with `ConsumableNamesMap`
(const -> name) into a concrete {id: name} map.

The result is keyed on the friendly version (major.minor.patch). G:\\wows_binaries
is READ-ONLY: this script only ever reads from it.

Era coverage:
  - era-2 (~9.1.1 - 13.8.0): module is `scripts/ConsumablesConstants.pyc`.
  - era-1/era-3: ConsumablesConstants.pyc absent; falls back to scanning the zip's
    file list for candidate modules and deobfuscating each until one yields the map.

Output: JSON { friendly_version: { "0": "crashCrew", ... } } plus a per-version
status line on stderr.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

WOWSDEOB = r"G:\dev\wowsdeob\target\release\wowsdeob.exe"
WOWS_BINARIES = r"G:\wows_binaries"

# Candidate module basenames to try, in priority order, when locating the map.
CANDIDATE_BASENAMES = [
    "ConsumablesConstants",  # era-2 (~9.1.1 - 13.8.0)
    "ConsumableConstants",  # era-3 (~13.9+, module renamed to singular)
    "ConsumableSystem",
    "BattleConsumableSystem",
]


def friendly_and_build(dirname):
    """`9.10.0_3052606` -> ('9.10.0', '3052606')."""
    idx = dirname.rfind("_")
    return dirname[:idx], dirname[idx + 1 :]


def list_pyc(zip_path):
    with zipfile.ZipFile(zip_path) as zf:
        return [n for n in zf.namelist() if n.endswith(".pyc")]


def candidate_members(members):
    """Yield zip member paths whose basename matches a known candidate, priority first."""
    by_base = {}
    for m in members:
        base = os.path.basename(m)[:-4]  # strip .pyc
        by_base.setdefault(base, []).append(m)
    for base in CANDIDATE_BASENAMES:
        for m in by_base.get(base, []):
            yield m
    # last resort: any module whose name contains "Consumable"
    for m in members:
        if "Consumable" in os.path.basename(m) and os.path.basename(m)[:-4] not in CANDIDATE_BASENAMES:
            yield m


def deob_member(zip_path, member, workdir):
    """Extract one .pyc from the zip and full-deobfuscate it. Return stage4 .py text or None."""
    with zipfile.ZipFile(zip_path) as zf:
        data = zf.read(member)
    base = os.path.basename(member)
    pyc_path = os.path.join(workdir, base)
    with open(pyc_path, "wb") as f:
        f.write(data)
    outdir = os.path.join(workdir, "out")
    os.makedirs(outdir, exist_ok=True)
    try:
        subprocess.run(
            [WOWSDEOB, "-q", pyc_path, outdir],
            check=False,
            timeout=120,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except subprocess.TimeoutExpired:
        return None
    deob = os.path.join(outdir, base[:-4] + "_stage4_deob.py")
    if not os.path.exists(deob):
        return None
    with open(deob, "r", encoding="utf-8", errors="replace") as f:
        return f.read()


def parse_types(text):
    """Parse the ConsumablesTypes enum -> {const_name: id}."""
    m = re.search(r"class ConsumablesTypes\b.*?(?=\nclass |\Z)", text, re.S)
    block = m.group(0) if m else None
    if not block:
        return {}
    start = 0
    sm = re.search(r"idGenerator\((\d+)\)", block)
    if sm:
        start = int(sm.group(1))
    ids = {}
    counter = start
    for line in block.splitlines():
        lm = re.match(r"\s*(CONSUMABLE_\w+)\s*=\s*(.+?)\s*$", line)
        if not lm:
            continue
        name, rhs = lm.group(1), lm.group(2).strip()
        if "next(enum)" in rhs:
            ids[name] = counter
            counter += 1
        elif re.fullmatch(r"-?\d+", rhs):
            ids[name] = int(rhs)
            counter = int(rhs) + 1
        # tuples / aliases (SPECIAL, CONSUMABLES_SURROGATE, etc.) are skipped
    return ids


def parse_name_class(text):
    """Parse `class ConsumableNames` -> {ATTR: 'value'} (newer indirection layer)."""
    m = re.search(r"class ConsumableNames\b.*?(?=\nclass |\Z)", text, re.S)
    if not m:
        return {}
    return dict(re.findall(r"(\w+)\s*=\s*'([^']+)'", m.group(0)))


def parse_names(text):
    """Parse ConsumableNamesMap -> {const_name: friendly_name}.

    Two forms occur across versions:
      9.x:   ConsumablesTypes.CONSUMABLE_X: 'crashCrew'         (string literal)
      12.x+: ConsumablesTypes.CONSUMABLE_X: ConsumableNames.CRASH_CREW  (class attr)
    """
    m = re.search(r"ConsumableNamesMap\s*=\s*\{(.*?)\}", text, re.S)
    if not m:
        return {}
    name_class = parse_name_class(text)
    out = {}
    for const, ref in re.findall(
        r"(?:ConsumablesTypes\.)?(CONSUMABLE_\w+)\s*:\s*([^,}]+)", m.group(1)
    ):
        ref = ref.strip()
        lit = re.fullmatch(r"'([^']+)'", ref)
        if lit:
            out[const] = lit.group(1)
            continue
        attr = re.fullmatch(r"ConsumableNames\.(\w+)", ref)
        if attr and attr.group(1) in name_class:
            out[const] = name_class[attr.group(1)]
    return out


def deob_full_zip(zip_path, workdir):
    """Deobfuscate an entire scripts.zip. Return the scripts output dir, or None."""
    outdir = os.path.join(workdir, "fullout")
    os.makedirs(outdir, exist_ok=True)
    try:
        subprocess.run(
            [WOWSDEOB, "-q", zip_path, outdir],
            check=False,
            timeout=900,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except subprocess.TimeoutExpired:
        return None
    scripts = os.path.join(outdir, "scripts")
    return scripts if os.path.isdir(scripts) else outdir


def find_types_in_dir(scripts_dir):
    """Scan deob output for the module defining `class ConsumablesTypes`; parse it."""
    for root, _dirs, files in os.walk(scripts_dir):
        for fn in files:
            if not fn.endswith("_stage4_deob.py"):
                continue
            path = os.path.join(root, fn)
            with open(path, "r", encoding="utf-8", errors="replace") as f:
                text = f.read()
            if "class ConsumablesTypes" not in text:
                continue
            types = parse_types(text)
            if types:
                return types, parse_names(text), fn[: -len("_stage4_deob.py")]
    return None, None, None


def extract_version(version_dir, workdir, full_fallback=False):
    """Return (types {const: id}, names {const: name}, status).

    `types` comes from the first candidate module that defines `class ConsumablesTypes`;
    `names` is whatever that same module exposes (may be empty -- the caller fills gaps
    from the canonical, version-stable const->name dict). When candidates miss and
    `full_fallback` is set, the whole zip is deobfuscated and scanned (slow; used for
    eras where the defining module has an obfuscated/renamed filename).
    """
    zip_path = os.path.join(WOWS_BINARIES, version_dir, "scripts.zip")
    if not os.path.exists(zip_path):
        return None, None, "no scripts.zip"
    try:
        members = list_pyc(zip_path)
    except zipfile.BadZipFile:
        return None, None, "bad zip"
    tried = []
    for member in candidate_members(members):
        tried.append(os.path.basename(member))
        text = deob_member(zip_path, member, workdir)
        if not text:
            continue
        types = parse_types(text)
        if types:
            return types, parse_names(text), os.path.basename(member)
    if full_fallback:
        scripts_dir = deob_full_zip(zip_path, workdir)
        if scripts_dir:
            types, names, module = find_types_in_dir(scripts_dir)
            if types:
                return types, names, f"{module} (full-scan)"
    return None, None, "no ConsumablesTypes in candidates: " + ",".join(tried[:5])


def main():
    args = [a for a in sys.argv[1:] if a != "--full"]
    full_fallback = "--full" in sys.argv
    out_path = args[0] if len(args) > 0 else os.path.join(".scratch", "consumable_ids.json")
    only = args[1] if len(args) > 1 else None  # optional substring filter
    dirs = sorted(
        d
        for d in os.listdir(WOWS_BINARIES)
        if re.match(r"^\d", d) and os.path.isdir(os.path.join(WOWS_BINARIES, d))
    )
    raw = {}  # friendly -> {build, types, names}
    canonical = {}  # const -> name, merged across every version that exposes names
    conflicts = []
    with tempfile.TemporaryDirectory() as workroot:
        for d in dirs:
            if only and only not in d:
                continue
            friendly, build = friendly_and_build(d)
            wd = os.path.join(workroot, d)
            os.makedirs(wd, exist_ok=True)
            try:
                types, names, status = extract_version(d, wd, full_fallback=full_fallback)
            finally:
                # Whole-zip deobfuscation output is large; reclaim it per version so the
                # temp volume does not fill across a full --full sweep.
                shutil.rmtree(wd, ignore_errors=True)
            if types:
                raw[friendly] = {"build": build, "types": types, "names": names or {}}
                for c, n in (names or {}).items():
                    if c in canonical and canonical[c] != n:
                        conflicts.append((c, canonical[c], n, friendly))
                    canonical[c] = n
                sys.stderr.write(
                    f"OK   {friendly:<14} ({len(types)} types, {len(names or {})} names) via {status}\n"
                )
            else:
                sys.stderr.write(f"MISS {friendly:<14} {status}\n")
            sys.stderr.flush()

    for c, a, b, v in conflicts:
        sys.stderr.write(f"WARN const {c} maps to both {a!r} and {b!r} (at {v})\n")

    result = {}
    missing_names = set()
    for friendly, v in raw.items():
        names = {**canonical, **v["names"]}  # prefer the version's own names
        id_map = {}
        for const, cid in v["types"].items():
            if const in names:
                id_map[str(cid)] = names[const]
            elif not const.startswith(("CONSUMABLE_UNUSED",)):
                missing_names.add(const)
        result[friendly] = {"build": v["build"], "ids": {k: id_map[k] for k in sorted(id_map, key=int)}}

    if missing_names:
        sys.stderr.write(f"\nconsts with no canonical name (skipped): {sorted(missing_names)}\n")
    os.makedirs(os.path.dirname(out_path) or ".", exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=2)
    sys.stderr.write(f"\nwrote {out_path} ({len(result)} versions)\n")


if __name__ == "__main__":
    main()
