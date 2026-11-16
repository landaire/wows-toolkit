#!/usr/bin/env python3
"""Backfill missing gettext catalogs into already-dumped builds.

Older builds were archived without their translations because earlier runs of
bulk_archive.py never fetched depot 552994 (the localization depot), where those
builds keep their `.mo` files. This script finds every archived build that is
missing `translations/en/.../global.mo`, downloads just that build's catalogs from
552994, and drops them into the dump's `translations/` tree (and the toolkit cache
if a copy lives there too). No map/pkg re-download.

Build -> 552994-manifest mapping: build numbers are ~linear in time, so we estimate
a date from the build number (anchored on known build/date pairs), pick the closest
localization manifest, and -- since the estimate can be off -- widen to nearby
manifests until the build's `.mo` actually come down.

Run: python scripts/fix_translations.py [--archive G:\\wows_builds] [--limit N]
"""
import argparse
import datetime as dt
import glob
import os
import re
import shutil
import subprocess
import sys
import tempfile

STEAMROOM = r"G:\dev\depotdownloader-rs-mit\target\release\steamroom.exe"
APP_ID = "552990"
LOC_DEPOT = "552994"
MANIFESTS_FILE = os.path.join(os.path.dirname(__file__), "manifests_552994_raw.txt")
CACHE_DIR = os.path.expandvars(r"%APPDATA%\WoWs Toolkit\data\game_data")

# (build_number, date) anchors taken from dated replays of those builds.
ANCHORS = [(3052606, dt.date(2020, 10, 27)), (5915585, dt.date(2022, 7, 13)), (7499736, dt.date(2023, 8, 5))]


def est_date(build):
    """Estimate a build's release date by linear interpolation over the anchors."""
    (b0, d0), (b1, d1) = ANCHORS[0], ANCHORS[-1]
    days = (d1 - d0).days * (build - b0) / (b1 - b0)
    return d0 + dt.timedelta(days=days)


def parse_manifests(path):
    """Return [(date, manifest_id)] from a `DD Month YYYY - ... <id>` list."""
    out = []
    for line in open(path, encoding="utf-8"):
        m = re.match(r"(\d+)\s+(\w+)\s+(\d{4}).*?\t(\d{8,})", line.strip())
        if not m:
            continue
        day, month, year, mid = m.groups()
        try:
            d = dt.datetime.strptime(f"{day} {month} {year}", "%d %B %Y").date()
        except ValueError:
            continue
        out.append((d, mid))
    return out


def missing_builds(archive):
    out = []
    for d in sorted(os.listdir(archive)):
        if not re.match(r"^\d", d) or "_" not in d:
            continue
        if not os.path.isdir(os.path.join(archive, d)):
            continue
        if not os.path.exists(os.path.join(archive, d, "translations", "en", "LC_MESSAGES", "global.mo")):
            out.append(d)
    return out


def download_mo(build, manifest, workdir):
    """Download just `build`'s catalogs from `manifest`. Return the res/texts dir, or None."""
    flt = os.path.join(workdir, "filter.txt")
    with open(flt, "w", encoding="utf-8", newline="\n") as f:
        f.write(rf"regex:bin[/\\]{build}[/\\]res[/\\]texts[/\\].*[/\\]LC_MESSAGES[/\\]global\.mo$" + "\n")
    out = os.path.join(workdir, "dl")
    subprocess.run(
        [STEAMROOM, "--use-steam-token", "--non-interactive", "--quiet", "download", "--app", APP_ID,
         "--depot", LOC_DEPOT, "--manifest", manifest, "--filelist", flt, "-o", out],
        check=False, timeout=600,
    )
    texts = os.path.join(out, "bin", str(build), "res", "texts")
    return texts if os.path.isdir(texts) and glob.glob(os.path.join(texts, "*", "LC_MESSAGES", "global.mo")) else None


def place(texts_dir, dest_build_dir):
    """Copy <lang>/LC_MESSAGES/global.mo from a res/texts dir into <dump>/translations/."""
    dest = os.path.join(dest_build_dir, "translations")
    for lang in os.listdir(texts_dir):
        src = os.path.join(texts_dir, lang, "LC_MESSAGES", "global.mo")
        if os.path.exists(src):
            d = os.path.join(dest, lang, "LC_MESSAGES")
            os.makedirs(d, exist_ok=True)
            shutil.copy2(src, os.path.join(d, "global.mo"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--archive", default=r"G:\wows_builds")
    ap.add_argument("--limit", type=int, default=0, help="only process the first N missing builds")
    ap.add_argument("--min-build", type=int, default=0, help="skip builds older than this number")
    ap.add_argument("--widen", type=int, default=6, help="how many nearby manifests to try per build")
    args = ap.parse_args()

    manifests = parse_manifests(MANIFESTS_FILE)
    miss = missing_builds(args.archive)
    if args.min_build:
        miss = [m for m in miss if int(m.rsplit("_", 1)[1]) >= args.min_build]
    if args.limit:
        miss = miss[: args.limit]
    print(f"{len(miss)} builds missing translations\n")

    fixed, failed = [], []
    with tempfile.TemporaryDirectory() as workroot:
        for name in miss:
            build = int(name.rsplit("_", 1)[1])
            target = est_date(build)
            # Localization manifests ranked by closeness to the estimated date.
            ranked = sorted(manifests, key=lambda m: abs((m[0] - target).days))
            wd = os.path.join(workroot, name)
            os.makedirs(wd, exist_ok=True)
            got = None
            for date, mid in ranked[: args.widen]:
                texts = download_mo(build, mid, wd)
                if texts:
                    got = (texts, date, mid)
                    break
            if not got:
                print(f"MISS {name}: no catalogs found in {args.widen} nearby manifests")
                failed.append(name)
                continue
            texts, date, mid = got
            place(texts, os.path.join(args.archive, name))
            cache_dir = os.path.join(CACHE_DIR, name)
            if os.path.isdir(cache_dir):
                place(texts, cache_dir)
            langs = len(glob.glob(os.path.join(texts, "*", "LC_MESSAGES", "global.mo")))
            print(f"OK   {name}: {langs} langs from {date} manifest {mid[:12]}")
            fixed.append(name)
            shutil.rmtree(wd, ignore_errors=True)

    print(f"\nfixed {len(fixed)}, failed {len(failed)}")
    if failed:
        print("failed:", failed)


if __name__ == "__main__":
    main()
