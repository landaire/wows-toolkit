# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""
Bulk download and archive all World of Warships game versions.

Usage:
    uv run scripts/bulk_archive.py [--dry-run] [--start-from N] [--no-skip] [-y]
    uv run scripts/bulk_archive.py --armor-only [--install-dir <install>] [--no-skip] [-y]
    uv run scripts/bulk_archive.py --scripts-only [--scripts-from <dir>] [--install-dir <install>] [--no-skip] [-y]

Requires:
    - steamroom built and on PATH (or pointed at by $STEAMROOM_BIN, or built in a
      sibling `steamroom` checkout beside this repo).
    - wows-data-mgr and wowsunpack built: `cargo build --release`.
    - Steam credentials (steamroom prompts interactively on first run) and a
      `.steam-user` file in the repo root containing your Steam username.
    - `gh` logged in with permission to upload release assets to
      landaire/wows-replay-data. Each archived build's client scripts.zip is
      published there as an asset of the `scripts` release.

The archive directory defaults to `wows_builds` beside this repo; override it
with $WOWS_BUILDS_DIR.

Steam App 552990 uses multiple depots:
    552993 - Client (~59 GiB): base idx + pkg, exe, scripts
    552991 - Content (~20 GiB): map (spaces/) idx + pkg, other assets
    552994 - Localizations (~252 MiB): translation .mo files

Maps live in the content depot (552991), so both depots are needed. To avoid
downloading tens of GiB, we fetch all idx (small) from both depots first, ask
`wowsunpack pkgs` for the minimal set of .pkg files that satisfies the dump's
required paths, then download only those packages.

Each WoWs update ships TWO builds (current + previous version), so we skip every
other major manifest and dump both builds from one download.

Downloads proceed oldest-to-newest so steamroom can delta-patch from the previous
manifest, saving bandwidth.
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
import zipfile
from datetime import datetime
from pathlib import Path

# Tool output (and the errors we echo from it) contains box-drawing characters
# that the default cp1252 stdout on Windows can't encode, which would otherwise
# crash the whole run on a single build's error message. Force utf-8 with
# replacement so a print can never abort the driver.
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

# This repo's root (the parent of scripts/).
REPO_ROOT = Path(__file__).resolve().parent.parent
EXE = ".exe" if os.name == "nt" else ""


def find_steamroom() -> Path:
    """Locate the steamroom binary, in order: $STEAMROOM_BIN, then PATH, then a
    sibling `steamroom` checkout's release build beside this repo."""
    if override := os.environ.get("STEAMROOM_BIN"):
        return Path(override)
    if on_path := shutil.which("steamroom"):
        return Path(on_path)
    return REPO_ROOT.parent / "steamroom" / "target" / "release" / f"steamroom{EXE}"


# Host-built workspace tools.
STEAMROOM = find_steamroom()
WOWS_DATA_MGR = REPO_ROOT / "target" / "release" / f"wows-data-mgr{EXE}"
WOWSUNPACK = REPO_ROOT / "target" / "release" / f"wowsunpack{EXE}"

# Where dumped builds are archived. Override with $WOWS_BUILDS_DIR; defaults to a
# `wows_builds` directory beside the repo. TEMP_DIR holds raw downloads (tens of
# GiB) before dumping, so it shares the archive's volume.
ARCHIVE_DIR = Path(os.environ.get("WOWS_BUILDS_DIR") or REPO_ROOT.parent / "wows_builds")
TEMP_DIR = ARCHIVE_DIR / "temp_game_data"

APP_ID = 552990
CLIENT_DEPOT = 552993
LOCALIZATION_DEPOT = 552994
CONTENT_DEPOT = 552991

# Manifest lists for each depot (oldest first after parsing).
CLIENT_MANIFESTS_FILE = REPO_ROOT / "scripts" / "manifests_raw.txt"
LOCALIZATION_MANIFESTS_FILE = REPO_ROOT / "scripts" / "manifests_552994_raw.txt"
CONTENT_MANIFESTS_FILE = REPO_ROOT / "scripts" / "manifests_552991_raw.txt"
# Per-build depot manifest ids.
GAME_VERSIONS_FILE = REPO_ROOT / "game_versions.toml"


def parse_date(s: str) -> datetime | None:
    clean = s.replace(" – ", " ").replace(" UTC", "")
    try:
        return datetime.strptime(clean, "%d %B %Y %H:%M:%S")
    except ValueError:
        return None


def parse_manifests(path: Path) -> list[tuple[str, str]]:
    """Parse a `<date>\\t<manifest_id>` list file into [(date_str, manifest_id)],
    oldest first.

    The source files are not guaranteed to be in clean chronological order (they
    are pasted from SteamDB and may be appended to out of order), so sort by the
    parsed date rather than trusting file order -- an unsorted file fed to
    filter_major_manifests silently drops whole patches.
    """
    entries = []
    for line in path.read_text(encoding="utf-8").strip().split("\n"):
        parts = line.split("\t")
        if len(parts) != 2 or parse_date(parts[0].strip()) is None:
            continue
        entries.append((parts[0].strip(), parts[1].strip()))
    entries.sort(key=lambda e: parse_date(e[0]))  # oldest first
    return entries


def filter_major_manifests(entries: list[tuple[str, str]]) -> list[tuple[str, str]]:
    """Keep only manifests >= 20 days apart (major version boundaries)."""
    filtered = []
    last_date = None
    for date_str, manifest in entries:
        dt = parse_date(date_str)
        if dt is None:
            continue
        if last_date is None or (dt - last_date).days >= 20:
            filtered.append((date_str, manifest))
            last_date = dt
    return filtered


def find_closest_manifest(target_date: datetime, manifest_list: list[tuple[str, str]],
                          max_days: int = 2) -> str | None:
    """Find the manifest closest in time to target_date, within max_days."""
    best_mid, best_diff = None, None
    for date_str, mid in manifest_list:
        dt = parse_date(date_str)
        if dt is None:
            continue
        diff = abs((target_date - dt).total_seconds())
        if best_diff is None or diff < best_diff:
            best_mid, best_diff = mid, diff
    if best_mid is not None and best_diff < 86400 * max_days:
        return best_mid
    return None


def detect_builds(game_dir: Path) -> list[int]:
    """Build numbers in game_dir/bin/ that have idx files."""
    bin_dir = game_dir / "bin"
    if not bin_dir.exists():
        return []
    builds = []
    for entry in bin_dir.iterdir():
        idx_dir = entry / "idx"
        if entry.is_dir() and entry.name.isdigit() and idx_dir.exists() and any(idx_dir.iterdir()):
            builds.append(int(entry.name))
    return sorted(builds)


def find_dump_dir(build: int) -> Path | None:
    """The archived dump directory for `build` (named `<version>_<build>`), if any."""
    if not ARCHIVE_DIR.exists():
        return None
    for entry in ARCHIVE_DIR.iterdir():
        if entry.is_dir() and entry.name.endswith(f"_{build}"):
            return entry
    return None


def already_archived(build: int) -> bool:
    """True if this build already has a dump directory with metadata."""
    dump = find_dump_dir(build)
    return dump is not None and (dump / "metadata.toml").exists()


# A real WoWS version ships dozens of battle maps. Partial/old dumps had 0-8
# minimaps; fully redumped builds have 35+. A build is only treated complete when
# it clears this floor, so a partially downloaded build is re-completed rather
# than skipped. Wrongly-not-skipped just costs a re-download; wrongly skipped
# would leave a build permanently broken, so bias toward re-doing.
MIN_COMPLETE_MAPS = 15


def map_count(build: int) -> int:
    """Number of extracted minimap images for this build (0 if absent)."""
    dump = find_dump_dir(build)
    spaces = dump / "vfs" / "spaces" if dump else None
    return sum(1 for _ in spaces.glob("*/minimap.png")) if spaces and spaces.exists() else 0


def has_translations(build: int) -> bool:
    """True if this build's dump carries its gettext catalogs (at least English)."""
    dump = find_dump_dir(build)
    return dump is not None and (dump / "translations" / "en" / "LC_MESSAGES" / "global.mo").exists()


def already_complete(build: int) -> bool:
    """True if this build's dump has a healthy minimap set and its translations.

    Translations are required: older builds keep their .mo only in the localization
    depot (552994), which earlier runs of this script never fetched, leaving the
    dumps unable to localize ship/skill/module names.
    """
    return map_count(build) >= MIN_COMPLETE_MAPS and has_translations(build)


def run_steamroom(depot: int, manifest: str, output: Path, filelist: Path, timeout: int = 3600,
                  priority: bool = False) -> bool:
    """Download a depot through the shared steamroom daemon. Returns True on success.
    `priority` puts the request ahead of queued ones, for small downloads that
    would otherwise wait behind another run's multi-GiB downloads."""
    # steamroom's delta-update deletes files from the previously-installed manifest
    # that aren't in the new one. We download many different manifests into the same
    # temp dir, so that would wrongly remove other builds' idx files (complete-build
    # then fails "idx directory not found"). Drop the per-depot install record before
    # each download so no delta-removal fires; files already on disk are untouched.
    (output / ".depotdownloader" / "depot.json").unlink(missing_ok=True)
    # Route every download through the long-lived daemon (started by ensure_daemon)
    # via --use-daemon. The daemon holds one authenticated Steam session and CDN
    # connection pool for the whole run, avoiding the per-process re-login and CDN
    # re-handshake that caused constant 503 rate limiting; its saved refresh token
    # also survives the host token expiring mid-run. --no-progress keeps the
    # streamed progress bar out of the log.
    cmd = [
        str(STEAMROOM), "--use-daemon", "--no-progress",
        "download",
        "--app", str(APP_ID),
        "--depot", str(depot),
        "--manifest", manifest,
        "--output", str(output),
        "--filelist", str(filelist),
        "--max-downloads", "4",
    ]
    if priority:
        cmd.insert(2, "--priority")
    # A download can still fail transiently (CDN 503s, a chunk error). Retry a few
    # times with growing backoff so a single pass heals builds that would otherwise
    # need another full pass.
    attempts = 5
    for attempt in range(attempts):
        if attempt > 0:
            backoff = min(30 * attempt, 120)
            print(f"  Retry attempt {attempt}/{attempts - 1} (sleep {backoff}s)...")
            time.sleep(backoff)
        try:
            result = subprocess.run(cmd, timeout=timeout)
        except subprocess.TimeoutExpired:
            print("  DOWNLOAD TIMED OUT")
            continue
        if result.returncode == 0:
            return True
        print(f"  DOWNLOAD FAILED (exit {result.returncode})")
    return False


def daemon_running() -> bool:
    """True if a steamroom daemon is up and answering RPC."""
    try:
        # `daemon status` without --text opens an interactive TUI that would hang
        # here; --text contacts the daemon and exits non-zero if none.
        res = subprocess.run(
            [str(STEAMROOM), "daemon", "status", "--text"],
            capture_output=True, text=True, timeout=30,
        )
        return res.returncode == 0
    except (subprocess.TimeoutExpired, OSError):
        return False


def ensure_daemon(steam_user: str):
    """Start a steamroom daemon (authenticated with the host Steam token) unless one
    is already running. All downloads run through it, so a single Steam session and
    CDN connection pool is reused for the whole archive run."""
    if daemon_running():
        print("steamroom daemon already running")
        return
    print(f"Starting steamroom daemon for {steam_user} (host Steam token)...")
    # Eager auth: --use-steam-token logs in with the host's cached token, saves a
    # durable refresh token, then forks a detached background daemon. Steam must be
    # running and logged in for this one-time login. Pass -u explicitly so the
    # daemon is bound to this account rather than a lazily-resolved one.
    res = subprocess.run(
        [str(STEAMROOM), "-u", steam_user, "--use-steam-token", "--non-interactive", "daemon", "start"],
        timeout=180,
    )
    if res.returncode != 0 or not daemon_running():
        print("ERROR: failed to start steamroom daemon. Is Steam running and logged in? "
              "Check the steamroom daemon log.")
        sys.exit(1)


def run_tool(args: list, timeout: int = 600) -> subprocess.CompletedProcess:
    """Run a host-built workspace Rust tool, capturing output."""
    return subprocess.run(
        [str(a) for a in args],
        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout,
    )


_REQUIRED_GLOBS: list[str] | None = None


def get_required_globs() -> list[str]:
    """The dump's required VFS path globs (cached; identical for every build)."""
    global _REQUIRED_GLOBS
    if _REQUIRED_GLOBS is None:
        res = run_tool([WOWS_DATA_MGR, "required-paths"], timeout=300)
        if res.returncode != 0:
            raise RuntimeError(f"required-paths failed: {res.stderr.strip()}")
        _REQUIRED_GLOBS = [s for line in res.stdout.splitlines() if (s := line.strip())]
    return _REQUIRED_GLOBS


def resolve_pkgs(build: int, globs: list[str], game_dir: Path = TEMP_DIR) -> dict:
    """Resolve the minimal set of .pkg files holding `globs` for `build`, from idx alone.

    Returns `wowsunpack pkgs --json` output: `pkgs` and `unmatched_patterns`."""
    idx_dir = game_dir / "bin" / str(build) / "idx"
    res = run_tool([WOWSUNPACK, "--idx-files", idx_dir, "pkgs", "--json", *globs])
    if res.returncode != 0:
        raise RuntimeError(
            f"pkg resolution failed for build {build} (rc={res.returncode}): "
            f"{(res.stderr or res.stdout).strip()[:300]}"
        )
    return json.loads(res.stdout)


_ARMOR_GLOBS: list[str] | None = None


def get_armor_globs() -> list[str]:
    """The VFS path globs `extract-armor` reads (cached; identical for every build)."""
    global _ARMOR_GLOBS
    if _ARMOR_GLOBS is None:
        res = run_tool([WOWS_DATA_MGR, "required-paths", "--armor"], timeout=300)
        if res.returncode != 0:
            raise RuntimeError(f"required-paths --armor failed: {res.stderr.strip()}")
        _ARMOR_GLOBS = [s for line in res.stdout.splitlines() if (s := line.strip())]
    return _ARMOR_GLOBS


# Extraction reads the ship prototypes from this file; builds without it have no
# armor to extract.
ASSETS_BIN = "content/assets.bin"

# Resume state for --armor-only: builds that cannot carry armor or failed
# extraction, and manifests whose builds are all settled. A build whose
# metadata.toml already lists armor/ entries is done regardless of this file.
ARMOR_PROGRESS_FILE = REPO_ROOT / "armor_backfill.json"


def load_armor_progress() -> dict:
    progress = {"no_assets_bin": [], "failed": {}, "manifests_done": []}
    if ARMOR_PROGRESS_FILE.exists():
        progress.update(json.loads(ARMOR_PROGRESS_FILE.read_text(encoding="utf-8")))
    return progress


def save_armor_progress(progress: dict):
    ARMOR_PROGRESS_FILE.write_text(json.dumps(progress, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def has_armor(build: int) -> bool:
    """True if this build's dump records the armor extract."""
    dump = find_dump_dir(build)
    meta = dump / "metadata.toml" if dump else None
    if meta is None or not meta.exists():
        return False
    return re.search(r'^"armor/', meta.read_text(encoding="utf-8"), re.MULTILINE) is not None


def armor_pending(build: int, progress: dict) -> bool:
    """True if `build` is archived, lacks armor, and has not been settled before."""
    return (
        already_archived(build)
        and not has_armor(build)
        and build not in progress["no_assets_bin"]
        and str(build) not in progress["failed"]
    )


def gib(size: int) -> str:
    return f"{size / 2**30:.2f} GiB"


def tree_size(path: Path) -> int:
    return sum(p.stat().st_size for p in path.rglob("*") if p.is_file()) if path.exists() else 0


def extract_build_armor(build: int, game_dir: Path, progress: dict):
    """Run `wows-data-mgr extract-armor` for `build` from `game_dir` into ARCHIVE_DIR
    and record a failure in `progress`. A timeout leaves the build pending."""
    print(f"    Extracting armor for build {build} from {game_dir}...")
    try:
        res = run_tool(
            [WOWS_DATA_MGR, "extract-armor", "--build", str(build), "--game-dir", game_dir, "--output", ARCHIVE_DIR],
            timeout=7200,
        )
    except subprocess.TimeoutExpired:
        print(f"    ARMOR EXTRACTION TIMED OUT for build {build}")
        return
    for line in (res.stdout or "").splitlines():
        if stripped := line.strip():
            print(f"      {stripped}")
    if res.returncode != 0:
        # stderr leads with per-model warnings; the error is at the end.
        error = (res.stderr or "").strip()[-2000:]
        print(f"    ARMOR EXTRACTION FAILED (exit {res.returncode}): {error[-800:]}")
        progress["failed"][str(build)] = error
    save_armor_progress(progress)


def armor_pkgs(build: int, game_dir: Path, progress: dict) -> list[str] | None:
    """The pkgs `extract-armor` reads for `build`, or None when they cannot be
    resolved or the build has no assets.bin (recorded in `progress`)."""
    try:
        data = resolve_pkgs(build, get_armor_globs(), game_dir)
    except (RuntimeError, json.JSONDecodeError, subprocess.TimeoutExpired) as e:
        print(f"    PKG RESOLUTION FAILED for build {build}: {e}")
        return None
    if ASSETS_BIN in data.get("unmatched_patterns", []):
        print(f"    Build {build}: no {ASSETS_BIN}, no armor to extract")
        progress["no_assets_bin"].append(build)
        save_armor_progress(progress)
        return None
    return data["pkgs"]


def extract_if_complete(build: int, pkgs: list[str], game_dir: Path, progress: dict):
    """Extract `build` when `game_dir` holds all of `pkgs`. A download that matched
    fewer files still succeeds, and the build is then left for another manifest."""
    pkg_dir = game_dir / "res_packages"
    missing = [p for p in pkgs if not (pkg_dir / p).exists()]
    if missing:
        print(f"    Build {build}: {game_dir} lacks {len(missing)} of {len(pkgs)} pkg(s); left pending")
        return
    size = sum((pkg_dir / p).stat().st_size for p in pkgs)
    print(f"    Build {build}: {len(pkgs)} pkg(s), {gib(size)}")
    extract_build_armor(build, game_dir, progress)


def backfill_armor_from_install(install: Path, progress: dict):
    """Extract armor for every pending build `install` holds completely, without
    downloading anything."""
    for build in sorted(detect_builds(install), reverse=True):
        if armor_pending(build, progress) and (pkgs := armor_pkgs(build, install, progress)) is not None:
            extract_if_complete(build, pkgs, install, progress)


def armor_manifest_candidates(download_list: list[tuple[str, str]],
                              content_manifests: list[tuple[str, str]]) -> list[tuple[str, str, str]]:
    """(label, client manifest, content manifest) to search for pending builds,
    newest first: the manifests game_versions.toml records per build, then the
    dated manifest lists. A manifest also carries the previous build's idx, which
    covers archived builds game_versions.toml does not list."""
    versions = tomllib.loads(GAME_VERSIONS_FILE.read_text(encoding="utf-8"))["versions"]
    candidates = []
    seen = set()
    for build, entry in sorted(versions.items(), key=lambda item: int(item[0]), reverse=True):
        pair = (entry.get("client_manifest_id"), entry.get("content_manifest_id"))
        if None not in pair and pair not in seen:
            seen.add(pair)
            candidates.append((f"{entry['version']} ({build})", *pair))
    for date_str, manifest_id in reversed(download_list):
        manifest_date = parse_date(date_str)
        content_manifest = find_closest_manifest(manifest_date, content_manifests) if manifest_date else None
        pair = (manifest_id, content_manifest)
        if content_manifest and pair not in seen:
            seen.add(pair)
            candidates.append((date_str, *pair))
    return candidates


def backfill_armor(candidates: list[tuple[str, str, str]], filelist_dir: Path, progress: dict):
    """Add the armor extract to archived builds that lack it. Per manifest pair,
    in order: fetch its idx, download only the pkgs `extract-armor` reads for its
    pending builds, extract them, then delete the downloads."""
    idx_filelist = filelist_dir / "idx.txt"
    idx_filelist.write_bytes(b"regex:\\.idx$\n")
    pkg_filelist = filelist_dir / "pkgs.txt"

    for label, client_manifest, content_manifest in candidates:
        key = f"{client_manifest}/{content_manifest}"
        if key in progress["manifests_done"]:
            continue
        print(f"\n{'=' * 60}")
        print(f"{label} - manifests {CLIENT_DEPOT}:{client_manifest} {CONTENT_DEPOT}:{content_manifest}")

        shutil.rmtree(TEMP_DIR, ignore_errors=True)
        TEMP_DIR.mkdir(parents=True)
        try:
            if not (run_steamroom(CLIENT_DEPOT, client_manifest, TEMP_DIR, idx_filelist, timeout=900)
                    and run_steamroom(CONTENT_DEPOT, content_manifest, TEMP_DIR, idx_filelist, timeout=900)):
                continue
            builds = detect_builds(TEMP_DIR)
            print(f"  Builds: {builds} (idx {gib(tree_size(TEMP_DIR))})")

            needed: dict[int, list[str]] = {}
            for build in sorted(builds, reverse=True):
                if armor_pending(build, progress) and (pkgs := armor_pkgs(build, TEMP_DIR, progress)) is not None:
                    needed[build] = pkgs

            if needed:
                union = sorted({p for pkgs in needed.values() for p in pkgs})
                write_pkg_filelist(union, pkg_filelist)
                print(f"  Downloading {len(union)} pkg(s) for builds {list(needed)}...")
                ok_client = run_steamroom(CLIENT_DEPOT, client_manifest, TEMP_DIR, pkg_filelist, timeout=7200)
                ok_content = run_steamroom(CONTENT_DEPOT, content_manifest, TEMP_DIR, pkg_filelist, timeout=7200)
                if not (ok_client and ok_content):
                    print("  PKG DOWNLOAD FAILED")
                    continue
                print(f"  Downloaded {gib(tree_size(TEMP_DIR / 'res_packages'))}")
                for build, pkgs in needed.items():
                    extract_if_complete(build, pkgs, TEMP_DIR, progress)

            if not any(armor_pending(build, progress) for build in builds):
                progress["manifests_done"].append(key)
                save_armor_progress(progress)
        finally:
            shutil.rmtree(TEMP_DIR, ignore_errors=True)

    print(f"\n{'=' * 60}")
    print(f"Builds still without armor: {armor_pending_dumps(progress) or 'none'}")
    print(f"Failed: {sorted(progress['failed'])}; without {ASSETS_BIN}: {len(progress['no_assets_bin'])}")


def armor_pending_dumps(progress: dict) -> list[str]:
    """Archived dump directories still lacking armor."""
    if not ARCHIVE_DIR.exists():
        return []
    return [
        entry.name for entry in sorted(ARCHIVE_DIR.iterdir())
        if entry.is_dir() and entry.name.rsplit("_", 1)[-1].isdigit()
        and armor_pending(int(entry.name.rsplit("_", 1)[-1]), progress)
    ]


# Client scripts.zip files do not deduplicate between builds, so they are
# published as assets of one release on the data repository instead of being
# committed. The release holds `<build>.zip` per build and an index.
SCRIPTS_REPO = "landaire/wows-replay-data"
SCRIPTS_RELEASE = "scripts"
SCRIPTS_INDEX = "index.toml"
# Downloads for --scripts-only, apart from TEMP_DIR so a concurrent run's
# cleanup cannot remove them.
SCRIPTS_TEMP_DIR = ARCHIVE_DIR / "temp_scripts"
# Loose file in the client depot, one per build the manifest carries.
SCRIPTS_ZIP_PATTERN = "bin[/\\\\]{build}[/\\\\]res[/\\\\]scripts\\.zip$"


def gh(*args: str, timeout: int = 1800) -> subprocess.CompletedProcess:
    """Run a `gh` command against the scripts release repository."""
    return subprocess.run(["gh", *args, "--repo", SCRIPTS_REPO], capture_output=True, text=True, timeout=timeout)


def gh_checked(*args: str, timeout: int = 1800) -> str:
    res = gh(*args, timeout=timeout)
    if res.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args)} failed (exit {res.returncode}): {res.stderr.strip()[:500]}")
    return res.stdout


def load_scripts_index(create: bool = True) -> dict[int, dict]:
    """The published scripts index as {build: {version, sha256, size}}. With
    `create`, creates the release when it does not exist."""
    releases = json.loads(gh_checked("release", "list", "--limit", "1000", "--json", "tagName"))
    if not any(release["tagName"] == SCRIPTS_RELEASE for release in releases):
        if create:
            gh_checked("release", "create", SCRIPTS_RELEASE, "--title", "Client scripts",
                       "--notes", f"Client scripts.zip per game build, from Steam depot {CLIENT_DEPOT}. "
                                  f"{SCRIPTS_INDEX} lists each build's version, sha256 and size.")
        return {}
    assets = json.loads(gh_checked("release", "view", SCRIPTS_RELEASE, "--json", "assets"))["assets"]
    if not any(asset["name"] == SCRIPTS_INDEX for asset in assets):
        # `--clobber` deletes the old index before uploading the new one, so a
        # failed upload leaves zips that only the lost index described.
        if any(asset["name"].removesuffix(".zip").isdigit() for asset in assets):
            raise RuntimeError(f"release {SCRIPTS_RELEASE} has build zips but no {SCRIPTS_INDEX}; restore it first")
        return {}
    text = gh_checked("release", "download", SCRIPTS_RELEASE, "--pattern", SCRIPTS_INDEX, "--output", "-")
    return {int(build): entry for build, entry in tomllib.loads(text)["builds"].items()}


def render_scripts_index(index: dict[int, dict]) -> str:
    lines = [f"# Client scripts.zip per game build, from Steam depot {CLIENT_DEPOT}.",
             f"# Release asset <build>.zip of release `{SCRIPTS_RELEASE}`.", ""]
    for build in sorted(index):
        entry = index[build]
        lines += [f"[builds.{build}]", f'version = "{entry["version"]}"', f'sha256 = "{entry["sha256"]}"',
                  f"size = {entry['size']}", ""]
    return "\n".join(lines)


def dump_version(build: int) -> str | None:
    """Game version of an archived build, from its dump directory name."""
    dump = find_dump_dir(build)
    return dump.name.rsplit("_", 1)[0] if dump else None


def scripts_pending(index: dict[int, dict]) -> list[int]:
    """Archived builds without a published scripts.zip, newest first."""
    if not ARCHIVE_DIR.exists():
        return []
    builds = [int(entry.name.rsplit("_", 1)[-1]) for entry in ARCHIVE_DIR.iterdir()
              if entry.is_dir() and entry.name.rsplit("_", 1)[-1].isdigit()]
    return sorted((b for b in builds if b not in index and already_archived(b)), reverse=True)


def publish_scripts(build: int, source: Path, index: dict[int, dict]) -> bool:
    """Upload `source` as the release asset `<build>.zip` and record it in the
    published index. Returns False when `source` is not a zip archive or the
    build has no dump to name its version."""
    version = dump_version(build)
    if version is None:
        print(f"    Build {build}: no archived dump, scripts.zip not published")
        return False
    if not zipfile.is_zipfile(source):
        print(f"    Build {build}: {source} is not a zip archive")
        return False

    SCRIPTS_TEMP_DIR.mkdir(parents=True, exist_ok=True)
    staged = SCRIPTS_TEMP_DIR / f"{build}.zip"
    if source.resolve() != staged.resolve():
        shutil.copyfile(source, staged)
    digest = hashlib.sha256()
    with open(staged, "rb") as f:
        while chunk := f.read(1 << 20):
            digest.update(chunk)
    size = staged.stat().st_size

    gh_checked("release", "upload", SCRIPTS_RELEASE, str(staged), "--clobber")
    index[build] = {"version": version, "sha256": digest.hexdigest(), "size": size}
    index_path = SCRIPTS_TEMP_DIR / SCRIPTS_INDEX
    index_path.write_text(render_scripts_index(index), encoding="utf-8")
    gh_checked("release", "upload", SCRIPTS_RELEASE, str(index_path), "--clobber")
    staged.unlink()
    print(f"    Build {build} ({version}): published scripts.zip, {size} bytes")
    return True


def scripts_manifest_candidates(download_list: list[tuple[str, str]]) -> list[tuple[str, str]]:
    """(label, client manifest) to search for scripts.zip files, newest first:
    the manifests game_versions.toml records per build, then the dated list."""
    versions = tomllib.loads(GAME_VERSIONS_FILE.read_text(encoding="utf-8"))["versions"]
    candidates = []
    seen = set()
    for build, entry in sorted(versions.items(), key=lambda item: int(item[0]), reverse=True):
        manifest = entry.get("client_manifest_id")
        if manifest and manifest not in seen:
            seen.add(manifest)
            candidates.append((f"{entry['version']} ({build})", manifest))
    for date_str, manifest in reversed(download_list):
        if manifest not in seen:
            seen.add(manifest)
            candidates.append((date_str, manifest))
    return candidates


def backfill_scripts(download_list: list[tuple[str, str]], scripts_from: Path | None, install: Path | None,
                     steam_user: str):
    """Publish scripts.zip for every archived build that lacks it: from
    `scripts_from/<build>.zip`, then from `install`, then from the client depot
    manifests, downloading only the pending builds' scripts.zip."""
    index = load_scripts_index()
    pending = scripts_pending(index)
    print(f"{len(index)} build(s) published, {len(pending)} pending")

    for build in list(pending):
        for source in ((scripts_from / f"{build}.zip") if scripts_from else None,
                       (install / "bin" / str(build) / "res" / "scripts.zip") if install else None):
            if source is not None and source.is_file() and publish_scripts(build, source, index):
                pending.remove(build)
                break
    if not pending:
        shutil.rmtree(SCRIPTS_TEMP_DIR, ignore_errors=True)
        return

    ensure_daemon(steam_user)
    filelist_dir = Path(tempfile.mkdtemp(prefix="bulk_archive_"))
    filelist = filelist_dir / "scripts.txt"
    try:
        for label, manifest in scripts_manifest_candidates(download_list):
            if not pending:
                break
            builds = "|".join(str(build) for build in pending)
            filelist.write_text(f"regex:{SCRIPTS_ZIP_PATTERN.format(build=f'({builds})')}\n", encoding="utf-8")
            print(f"\n{label} - manifest {CLIENT_DEPOT}:{manifest}")
            shutil.rmtree(SCRIPTS_TEMP_DIR, ignore_errors=True)
            SCRIPTS_TEMP_DIR.mkdir(parents=True)
            if not run_steamroom(CLIENT_DEPOT, manifest, SCRIPTS_TEMP_DIR, filelist, timeout=7200, priority=True):
                continue
            for build in list(pending):
                source = SCRIPTS_TEMP_DIR / "bin" / str(build) / "res" / "scripts.zip"
                if source.is_file() and publish_scripts(build, source, index):
                    pending.remove(build)
    finally:
        shutil.rmtree(filelist_dir, ignore_errors=True)
        shutil.rmtree(SCRIPTS_TEMP_DIR, ignore_errors=True)

    print(f"\n{'=' * 60}")
    print(f"Builds without a published scripts.zip: {pending or 'none'}")


def write_pkg_filelist(pkgs: list[str], path: Path):
    """Write a steamroom filelist matching exactly the given pkg names in res_packages."""
    lines = [f"regex:res_packages[/\\\\]{re.escape(p)}$" for p in pkgs]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def dump_build(build: int, completing: bool, timeout: int = 600) -> subprocess.CompletedProcess:
    """Run wows-data-mgr to dump (or complete) `build` from TEMP_DIR into ARCHIVE_DIR."""
    subcommand, flag = ("complete-build", "--with-gui") if completing else ("dump-renderer-data", "--force")
    return run_tool(
        [WOWS_DATA_MGR, subcommand, "--build", str(build),
         "--game-dir", TEMP_DIR, "--output", ARCHIVE_DIR, flag],
        timeout=timeout,
    )


def main():
    parser = argparse.ArgumentParser(description="Bulk archive WoWs game versions")
    parser.add_argument("--dry-run", action="store_true", help="Show what would be downloaded without doing it")
    parser.add_argument("--start-from", type=int, default=0, help="Start from manifest index N (0-based)")
    parser.add_argument("--no-skip", action="store_true", help="Download every major manifest instead of every other")
    parser.add_argument("-y", "--yes", action="store_true", help="Skip confirmation prompt")
    parser.add_argument("--armor-only", action="store_true",
                        help="Add the armor extract to archived builds that lack it, newest first, downloading "
                             "only the pkgs extraction reads")
    parser.add_argument("--install-dir", type=Path,
                        help="With --armor-only or --scripts-only: a game install whose builds are used first, "
                             "without downloading")
    parser.add_argument("--scripts-only", action="store_true",
                        help="Publish the client scripts.zip of archived builds that lack it, downloading only "
                             "scripts.zip from the client depot")
    parser.add_argument("--scripts-from", type=Path,
                        help="With --scripts-only: a directory of <build>.zip files used before the install and "
                             "the depot")
    args = parser.parse_args()
    if args.armor_only and args.scripts_only:
        parser.error("--armor-only and --scripts-only are exclusive")
    if args.install_dir and not (args.armor_only or args.scripts_only):
        parser.error("--install-dir requires --armor-only or --scripts-only")
    if args.scripts_from and not args.scripts_only:
        parser.error("--scripts-from requires --scripts-only")

    client_manifests = parse_manifests(CLIENT_MANIFESTS_FILE)
    major = filter_major_manifests(client_manifests)
    download_list = major if args.no_skip else major[::2]

    content_manifests = parse_manifests(CONTENT_MANIFESTS_FILE) if CONTENT_MANIFESTS_FILE.exists() else []
    localization_manifests = parse_manifests(LOCALIZATION_MANIFESTS_FILE) if LOCALIZATION_MANIFESTS_FILE.exists() else []
    if not content_manifests:
        print(f"ERROR: no content-depot manifests ({CONTENT_MANIFESTS_FILE}); maps live in depot "
              f"{CONTENT_DEPOT} and cannot be dumped without it.")
        sys.exit(1)
    if not localization_manifests:
        print(f"WARNING: no localization-depot manifests ({LOCALIZATION_MANIFESTS_FILE}); "
              f"older builds' translations (depot {LOCALIZATION_DEPOT}) will be missing.")

    print(f"Archive dir:           {ARCHIVE_DIR}")
    print(f"Total client manifests: {len(client_manifests)}")
    print(f"Major manifests:        {len(major)}")
    print(f"To download:            {len(download_list)}")
    print(f"Starting from index:    {args.start_from}")
    print()

    download_list = download_list[args.start_from:]
    for i, (date_str, manifest) in enumerate(download_list):
        print(f"  {i + args.start_from:3d}. {date_str:40s} {manifest}")

    if args.dry_run:
        if args.armor_only:
            pending = armor_pending_dumps(load_armor_progress())
            print(f"\n--dry-run: {len(pending)} archived build(s) lack armor: {pending}")
        elif args.scripts_only:
            pending = scripts_pending(load_scripts_index(create=False))
            print(f"\n--dry-run: {len(pending)} archived build(s) lack a published scripts.zip: {pending}")
        else:
            print("\n--dry-run: would download the above manifests")
        return

    for name, path in (("steamroom", STEAMROOM), ("wows-data-mgr", WOWS_DATA_MGR), ("wowsunpack", WOWSUNPACK)):
        if not path.exists():
            print(f"ERROR: {name} not found at {path}")
            print("Build the workspace tools with `cargo build --release` (and steamroom in its checkout).")
            sys.exit(1)

    steam_user_file = REPO_ROOT / ".steam-user"
    if not steam_user_file.exists():
        print(f"ERROR: no {steam_user_file} file. Create one containing your Steam username.")
        sys.exit(1)
    steam_user = steam_user_file.read_text().strip()

    if not args.yes:
        print()
        input("Press Enter to start, Ctrl+C to abort...")

    ARCHIVE_DIR.mkdir(parents=True, exist_ok=True)
    if args.armor_only:
        progress = load_armor_progress()
        if args.install_dir:
            backfill_armor_from_install(args.install_dir, progress)
        ensure_daemon(steam_user)
        filelist_dir = Path(tempfile.mkdtemp(prefix="bulk_archive_"))
        try:
            backfill_armor(armor_manifest_candidates(download_list, content_manifests), filelist_dir, progress)
        finally:
            shutil.rmtree(filelist_dir, ignore_errors=True)
        return
    if args.scripts_only:
        backfill_scripts(download_list, args.scripts_from, args.install_dir, steam_user)
        return

    ensure_daemon(steam_user)
    scripts_index = load_scripts_index()
    discovered = {}

    # idx files are small and index every packed file, so we fetch all of them from
    # both depots first, resolve the minimal pkg set, then download only those pkgs.
    # The idx filelist also pulls the per-build per-locale gettext catalogs (loose
    # files in the client depot under bin/<build>/res/texts) and each build's
    # scripts.zip. Filelists are scratch files, so keep them in a temp dir rather
    # than the repo.
    filelist_dir = Path(tempfile.mkdtemp(prefix="bulk_archive_"))
    try:
        idx_filelist = filelist_dir / "idx.txt"
        idx_filelist.write_bytes(
            b"regex:\\.idx$\n"
            b"regex:bin[/\\\\]\\d+[/\\\\]res[/\\\\]texts[/\\\\].*[/\\\\]LC_MESSAGES[/\\\\]global\\.mo$\n"
            + ("regex:" + SCRIPTS_ZIP_PATTERN.format(build="\\d+") + "\n").encode()
        )
        pkg_filelist = filelist_dir / "pkgs.txt"

        for i, (date_str, manifest_id) in enumerate(download_list):
            idx = i + args.start_from
            print(f"\n{'=' * 60}")
            print(f"[{idx + 1}] {date_str} - manifest {manifest_id}")

            manifest_date = parse_date(date_str)
            content_manifest = find_closest_manifest(manifest_date, content_manifests) if manifest_date else None
            if not content_manifest:
                print(f"  No content-depot ({CONTENT_DEPOT}) manifest near {date_str}; maps would be missing. Skipping.")
                continue

            TEMP_DIR.mkdir(parents=True, exist_ok=True)
            builds_before = set(detect_builds(TEMP_DIR))

            # --- Download idx from both depots (client base + content maps) ---
            print(f"  Downloading idx: client depot {CLIENT_DEPOT}...")
            if not run_steamroom(CLIENT_DEPOT, manifest_id, TEMP_DIR, idx_filelist, timeout=900):
                continue
            print(f"  Downloading idx: content depot {CONTENT_DEPOT} (manifest {content_manifest})...")
            if not run_steamroom(CONTENT_DEPOT, content_manifest, TEMP_DIR, idx_filelist, timeout=900):
                continue

            # The per-locale gettext catalogs (global.mo) live in the localization
            # depot. Older builds keep them ONLY there -- not in the client depot's
            # bin/<build>/res/texts -- so without this the toolkit can't localize
            # ship/skill/module names. Non-fatal: a build still dumps without it.
            loc_manifest = find_closest_manifest(manifest_date, localization_manifests) if manifest_date else None
            if loc_manifest:
                print(f"  Downloading translations: localization depot {LOCALIZATION_DEPOT} (manifest {loc_manifest})...")
                run_steamroom(LOCALIZATION_DEPOT, loc_manifest, TEMP_DIR, idx_filelist, timeout=900)
            else:
                print(f"  No localization-depot manifest near {date_str}; translations may be missing.")

            all_builds = detect_builds(TEMP_DIR)
            builds = sorted(set(all_builds) - builds_before)
            print(f"  Found builds: {builds} (new out of {len(all_builds)} in temp)")
            if not builds:
                print("  No new builds, skipping")
                continue

            # --- Per build: resolve minimal pkg set, download it, dump ---
            for build in builds:
                if already_complete(build):
                    print(f"    Build {build}: already complete ({map_count(build)} maps), skipping")
                    continue

                try:
                    data = resolve_pkgs(build, get_required_globs())
                    pkgs = data["pkgs"]
                    if unmatched := data.get("unmatched_patterns", []):
                        # Expected for version-specific dirs absent in this build; informational.
                        print(f"    note: {len(unmatched)} required glob(s) matched nothing in build {build}")
                except (RuntimeError, json.JSONDecodeError) as e:
                    print(f"    PKG RESOLUTION FAILED for build {build}: {e}")
                    continue

                # An existing-but-incomplete build already holds its GameParams.data,
                # entity defs, and most assets in the shared store; it only needs maps
                # and the newer gui dirs. Complete it in place and skip the multi-GiB
                # basecontent package whose GameParams.data we already have.
                completing = already_archived(build)
                if completing:
                    pkgs = [p for p in pkgs if not p.startswith("basecontent")]
                mode = "complete" if completing else "full dump"
                print(f"    Build {build}: {len(pkgs)} pkg(s) required ({mode})")
                write_pkg_filelist(pkgs, pkg_filelist)

                # The required pkgs are split across both depots; each download grabs
                # whatever subset it holds.
                print(f"    Downloading {len(pkgs)} pkg(s) from depots {CLIENT_DEPOT} + {CONTENT_DEPOT}...")
                ok_client = run_steamroom(CLIENT_DEPOT, manifest_id, TEMP_DIR, pkg_filelist, timeout=3600)
                ok_content = run_steamroom(CONTENT_DEPOT, content_manifest, TEMP_DIR, pkg_filelist, timeout=3600)
                if not (ok_client and ok_content):
                    print(f"    PKG DOWNLOAD FAILED for build {build}")
                    continue

                print(f"    {'Completing' if completing else 'Dumping'} build {build}...")
                result = dump_build(build, completing)
                if result.returncode != 0:
                    print(f"    DUMP FAILED (exit {result.returncode}):")
                    print(f"      stdout: {(result.stdout or '').strip()[:500]}")
                    print(f"      stderr: {(result.stderr or '').strip()[:800]}")
                    continue
                for line in (result.stderr or "").splitlines():
                    if stripped := line.strip():
                        print(f"      {stripped}")

                discovered[build] = {"manifest_id": manifest_id, "date": date_str}
                print("    OK")

            for build in builds:
                source = TEMP_DIR / "bin" / str(build) / "res" / "scripts.zip"
                if build not in scripts_index and source.is_file():
                    # A failed upload must not end the archive run; --scripts-only
                    # publishes the build later.
                    try:
                        publish_scripts(build, source, scripts_index)
                    except (RuntimeError, subprocess.TimeoutExpired) as e:
                        print(f"    SCRIPTS PUBLISH FAILED for build {build}: {e}")
    finally:
        shutil.rmtree(filelist_dir, ignore_errors=True)
        shutil.rmtree(SCRIPTS_TEMP_DIR, ignore_errors=True)
        if TEMP_DIR.exists():
            shutil.rmtree(TEMP_DIR, ignore_errors=True)

    print(f"\n{'=' * 60}")
    print(f"DONE! Archived {len(discovered)} new builds.")

    if discovered:
        out = REPO_ROOT / "game_versions_discovered.toml"
        with open(out, "w", encoding="utf-8") as f:
            f.write("# Discovered builds from bulk archive\n\n")
            for build in sorted(discovered):
                info = discovered[build]
                f.write(f"[versions.{build}]\n")
                f.write(f'# date = "{info["date"]}"\n')
                f.write(f"client_depot_id = {CLIENT_DEPOT}\n")
                f.write(f'client_manifest_id = "{info["manifest_id"]}"\n\n')
        print(f"Discovered builds written to {out}")


if __name__ == "__main__":
    main()
