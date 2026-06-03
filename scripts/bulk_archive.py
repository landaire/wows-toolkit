# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

"""
Bulk download and archive all World of Warships game versions.

Usage:
    uv run scripts/bulk_archive.py [--dry-run] [--start-from N] [--no-skip]

Requires:
    - steamroom built: cargo build --release (in G:\\dev\\depotdownloader-rs-mit)
    - wows-data-mgr + wowsunpack built: cargo build --release (via WSL nix)
    - Steam credentials (steamroom will prompt interactively on first run)

Steam App 552990 uses multiple depots:
    552993 - Client (~59 GiB): base idx + pkg, exe, scripts
    552991 - Content (~20 GiB): map (spaces/) idx + pkg, other assets
    552994 - Localizations (~252 MiB): translation .mo files

Maps live in the content depot (552991), so both depots are needed. To avoid
downloading tens of GiB, we fetch all idx (small) from both depots first, ask
`wowsunpack pkgs` for the minimal set of .pkg files that satisfies the dump's
required paths, then download only those packages.

Each WoWs update ships TWO builds (current + previous version), so we
skip every other major manifest and dump both builds from one download.

Downloads proceed oldest-to-newest so steamroom can delta-patch
from the previous manifest, saving bandwidth.
"""

import argparse
import json
import re
import shlex
import shutil
import subprocess
import sys
import time
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

TEMP_DIR = Path(r"G:\wows_builds\temp_game_data")
ARCHIVE_DIR = Path(r"G:\wows_builds")
APP_ID = 552990
CLIENT_DEPOT = 552993
LOCALIZATION_DEPOT = 552994
CONTENT_DEPOT = 552991
REPO_ROOT = Path(r"G:\dev\wows-toolkit")
STEAMROOM = Path(r"G:\dev\depotdownloader-rs-mit\target\release\steamroom.exe")

# Manifest lists for each depot (oldest first after parsing)
CLIENT_MANIFESTS_FILE = REPO_ROOT / "scripts" / "manifests_raw.txt"
LOCALIZATION_MANIFESTS_FILE = REPO_ROOT / "scripts" / "manifests_552994_raw.txt"
CONTENT_MANIFESTS_FILE = REPO_ROOT / "scripts" / "manifests_552991_raw.txt"

# Filelist for localization depot (depot 552994) - just .mo files
LOCALIZATION_FILELIST = REPO_ROOT / "scripts" / ".download_translations_only.tmp"


def parse_manifests(path: Path) -> list[tuple[str, str]]:
    """Parse a manifest list file. Returns [(date_str, manifest_id), ...] oldest
    first, sorted by parsed date.

    The source files are not guaranteed to be in clean chronological order (they
    are pasted from SteamDB and may be appended to out of order), so sort by the
    actual date rather than trusting file order -- an unsorted file fed to
    filter_major_manifests silently drops whole patches.
    """
    entries = []
    for line in path.read_text(encoding="utf-8").strip().split("\n"):
        parts = line.split("\t")
        if len(parts) != 2:
            continue
        date_str = parts[0].strip()
        if parse_date(date_str) is None:
            continue
        entries.append((date_str, parts[1].strip()))
    entries.sort(key=lambda e: parse_date(e[0]))  # oldest first
    return entries


def parse_date(s: str) -> datetime | None:
    clean = s.replace(" – ", " ").replace(" UTC", "")
    try:
        return datetime.strptime(clean, "%d %B %Y %H:%M:%S")
    except ValueError:
        return None


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
            best_diff = diff
            best_mid = mid
    if best_mid and best_diff is not None and best_diff < 86400 * max_days:
        return best_mid
    return None


def detect_builds(game_dir: Path) -> list[int]:
    """Find build numbers in game_dir/bin/ that have idx files."""
    bin_dir = game_dir / "bin"
    if not bin_dir.exists():
        return []
    builds = []
    for entry in bin_dir.iterdir():
        if entry.is_dir() and entry.name.isdigit():
            idx_dir = entry / "idx"
            if idx_dir.exists() and any(idx_dir.iterdir()):
                builds.append(int(entry.name))
    builds.sort()
    return builds


def already_archived(build: int) -> bool:
    """Check if this build already has a dump directory with metadata."""
    for entry in ARCHIVE_DIR.iterdir():
        if entry.is_dir() and entry.name.endswith(f"_{build}") and (entry / "metadata.toml").exists():
            return True
    return False


# A real WoWS version ships dozens of battle maps. Partial/old dumps had 0-8
# minimaps; fully redumped builds have 35+. A build is only treated complete
# when it clears this floor, so a partially downloaded build is re-completed
# rather than skipped. Wrongly-not-skipped just costs a re-download; wrongly
# skipped would leave a build permanently broken, so bias toward re-doing.
MIN_COMPLETE_MAPS = 15


def map_count(build: int) -> int:
    """Number of extracted minimap images for this build (0 if absent)."""
    for entry in ARCHIVE_DIR.iterdir():
        if entry.is_dir() and entry.name.endswith(f"_{build}"):
            spaces = entry / "vfs" / "spaces"
            if spaces.exists():
                return sum(1 for _ in spaces.glob("*/minimap.png"))
    return 0


def has_translations(build: int) -> bool:
    """True if this build's dump carries its gettext catalogs (at least English)."""
    for entry in ARCHIVE_DIR.iterdir():
        if entry.is_dir() and entry.name.endswith(f"_{build}"):
            return (entry / "translations" / "en" / "LC_MESSAGES" / "global.mo").exists()
    return False


def already_complete(build: int) -> bool:
    """True if this build's dump has a healthy minimap set and its translations.

    Translations are required: older builds keep their .mo only in the localization
    depot (552994), which earlier runs of this script never fetched, leaving the
    dumps unable to localize ship/skill/module names. Re-running now re-completes them.
    """
    return map_count(build) >= MIN_COMPLETE_MAPS and has_translations(build)


def run_steamroom(steam_user: str, depot: int, manifest: str, output: Path,
                  filelist: Path, timeout: int = 3600) -> bool:
    """Run a steamroom download through the shared daemon. Returns True on success."""
    # steamroom's delta-update deletes files from the previously-installed
    # manifest that aren't in the new one. We download many different manifests
    # into the same temp dir, so that wrongly removes other builds' idx files
    # (complete-build then fails "idx directory not found"). Drop the per-depot
    # install record before each download so no delta-removal fires; the idx and
    # package files already on disk are untouched.
    (output / ".depotdownloader" / "depot.json").unlink(missing_ok=True)
    # Route every download through the long-lived daemon (started by
    # ensure_daemon) via --use-daemon. The daemon holds one authenticated Steam
    # session and CDN connection pool for the whole run, so we avoid the
    # per-process re-login and CDN re-handshake that caused constant 503 rate
    # limiting, and its saved refresh token survives the host token expiring
    # mid-run. --no-progress keeps the streamed progress bar out of the log.
    cmd = [
        str(STEAMROOM),
        "--use-daemon",
        "--no-progress",
        "download",
        "--app", str(APP_ID),
        "--depot", str(depot),
        "--manifest", manifest,
        "--output", str(output),
        "--filelist", str(filelist),
        "--max-downloads", "4",
    ]
    # A download can still fail transiently (CDN 503s, a chunk error). The client
    # blocks until the daemon job finishes and exits with its exit code, so retry
    # a few times with growing backoff; this lets a single pass heal builds that
    # would otherwise need another full pass to retry.
    attempts = 5
    for attempt in range(attempts):
        if attempt > 0:
            backoff = min(30 * attempt, 120)
            print(f"  Retry attempt {attempt}/{attempts - 1} (sleep {backoff}s)...")
            time.sleep(backoff)
        try:
            result = subprocess.run(cmd, timeout=timeout)
        except subprocess.TimeoutExpired:
            print(f"  DOWNLOAD TIMED OUT")
            continue
        if result.returncode == 0:
            return True
        print(f"  DOWNLOAD FAILED (exit {result.returncode})")
    return False


def daemon_running() -> bool:
    """True if a steamroom daemon is up and answering RPC."""
    try:
        # `daemon status` without --text opens an interactive TUI that would
        # hang here; --text contacts the daemon and exits non-zero if none.
        res = subprocess.run(
            [str(STEAMROOM), "daemon", "status", "--text"],
            capture_output=True, text=True, timeout=30,
        )
        return res.returncode == 0
    except (subprocess.TimeoutExpired, OSError):
        return False


def ensure_daemon(steam_user: str):
    """Start a steamroom daemon (authenticated with the host Steam token) unless
    one is already running. All downloads run through it, so a single Steam
    session and CDN connection pool is reused for the whole archive run."""
    if daemon_running():
        print("steamroom daemon already running")
        return
    print(f"Starting steamroom daemon for {steam_user} (host Steam token)...")
    # Eager auth: --use-steam-token logs in with the host's cached token, saves a
    # durable refresh token, then forks a detached background daemon. Steam must
    # be running and logged in for this one-time login. Pass -u explicitly so the
    # daemon is bound to this account rather than a lazily-resolved one.
    res = subprocess.run(
        [str(STEAMROOM), "-u", steam_user, "--use-steam-token", "--non-interactive", "daemon", "start"],
        timeout=180,
    )
    if res.returncode != 0 or not daemon_running():
        print("ERROR: failed to start steamroom daemon. Is Steam running and "
              "logged in? See the daemon log under %LOCALAPPDATA%\\steamroom\\daemon.log")
        sys.exit(1)


WSL_REPO = "/mnt/g/dev/wows-toolkit"


def to_wsl(path: Path) -> str:
    """Convert a Windows path to its WSL /mnt form."""
    s = str(path).replace("\\", "/")
    if len(s) >= 2 and s[1] == ":":
        s = "/mnt/" + s[0].lower() + s[2:]
    return s


def run_wsl_tool(tool_args: str, timeout: int = 600) -> subprocess.CompletedProcess:
    """Run a workspace Rust tool inside the WSL nix dev shell, capturing output."""
    cmd = ["wsl", "bash", "-lc", f"cd {WSL_REPO} && nix develop --command {tool_args}"]
    return subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout)


_REQUIRED_GLOBS: list[str] | None = None


def get_required_globs() -> list[str]:
    """The dump's required VFS path globs (cached; same for every build)."""
    global _REQUIRED_GLOBS
    if _REQUIRED_GLOBS is None:
        # First wsl call of the run; a cold nix dev-shell eval can take ~30 min
        # before anything is cached, so allow far more than the default timeout.
        res = run_wsl_tool("./target/release/wows-data-mgr required-paths", timeout=2700)
        if res.returncode != 0:
            raise RuntimeError(f"required-paths failed: {res.stderr.strip()}")
        # `nix develop` may prepend "warning: Git tree ... is dirty" lines to
        # stdout. Globs never contain whitespace, so drop any line that does.
        _REQUIRED_GLOBS = [s for line in res.stdout.splitlines() if (s := line.strip()) and " " not in s]
    return _REQUIRED_GLOBS


def resolve_pkgs(build: int) -> list[str]:
    """Resolve the minimal set of .pkg files needed to dump `build`, from idx alone."""
    globs = get_required_globs()
    idx_wsl = f"{to_wsl(TEMP_DIR)}/bin/{build}/idx"
    quoted = " ".join(shlex.quote(g) for g in globs)
    res = run_wsl_tool(f"./target/release/wowsunpack --idx-files {idx_wsl} pkgs --json {quoted}")
    # Don't gate on returncode: a dirty working tree makes `nix develop` exit
    # non-zero while still producing valid JSON on stdout. Treat the run as
    # successful whenever parseable JSON is present, and only fail if it isn't.
    brace = res.stdout.find("{")
    if brace < 0:
        raise RuntimeError(
            f"pkg resolution failed for build {build} (rc={res.returncode}): "
            f"{(res.stderr or res.stdout).strip()[:300]}"
        )
    data = json.loads(res.stdout[brace:])
    unmatched = data.get("unmatched_patterns", [])
    if unmatched:
        # Expected for version-specific dirs absent in this build; informational.
        print(f"    note: {len(unmatched)} required glob(s) matched nothing in build {build}")
    return data["pkgs"]


def write_pkg_filelist(pkgs: list[str], path: Path):
    """Write a steamroom filelist matching exactly the given pkg names in res_packages."""
    lines = [f"regex:res_packages[/\\\\]{re.escape(p)}$" for p in pkgs]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description="Bulk archive WoWs game versions")
    parser.add_argument("--dry-run", action="store_true", help="Show what would be downloaded without doing it")
    parser.add_argument("--start-from", type=int, default=0, help="Start from manifest index N (0-based)")
    parser.add_argument("--no-skip", action="store_true", help="Download every major manifest instead of every other")
    parser.add_argument("-y", "--yes", action="store_true", help="Skip confirmation prompt")
    args = parser.parse_args()

    steam_user_file = REPO_ROOT / ".steam-user"
    if not steam_user_file.exists():
        print("ERROR: No .steam-user file. Create one with your Steam username.")
        sys.exit(1)
    steam_user = steam_user_file.read_text().strip()

    if not STEAMROOM.exists():
        print(f"ERROR: steamroom not found at {STEAMROOM}")
        print("Build it: cd G:\\dev\\depotdownloader-rs-mit && cargo build --release")
        sys.exit(1)

    # Parse manifest lists
    client_manifests = parse_manifests(CLIENT_MANIFESTS_FILE)
    major = filter_major_manifests(client_manifests)
    download_list = major if args.no_skip else major[::2]

    content_manifests = parse_manifests(CONTENT_MANIFESTS_FILE) if CONTENT_MANIFESTS_FILE.exists() else []
    localization_manifests = parse_manifests(LOCALIZATION_MANIFESTS_FILE) if LOCALIZATION_MANIFESTS_FILE.exists() else []
    if not localization_manifests:
        print(f"WARNING: no localization-depot manifests ({LOCALIZATION_MANIFESTS_FILE}); "
              f"older builds' translations (depot {LOCALIZATION_DEPOT}) will be missing.")
    if not content_manifests:
        print(f"ERROR: no content-depot manifests ({CONTENT_MANIFESTS_FILE}); maps live in depot "
              f"{CONTENT_DEPOT} and cannot be dumped without it.")
        sys.exit(1)

    print(f"Total client manifests: {len(client_manifests)}")
    print(f"Major manifests: {len(major)}")
    print(f"To download: {len(download_list)}")
    print(f"Starting from index: {args.start_from}")
    print()

    download_list = download_list[args.start_from:]

    for i, (date_str, manifest) in enumerate(download_list):
        idx = i + args.start_from
        print(f"  {idx:3d}. {date_str:40s} {manifest}")

    if args.dry_run:
        print("\n--dry-run: would download the above manifests")
        return

    if not args.yes:
        print()
        input("Press Enter to start, Ctrl+C to abort...")

    ensure_daemon(steam_user)

    ARCHIVE_DIR.mkdir(parents=True, exist_ok=True)
    discovered = {}

    # idx files are small and index every packed file, so we fetch all of them
    # from both depots first, resolve the minimal pkg set, then download only
    # those pkgs. The pkg filelist is regenerated per build.
    # idx (both depots) plus the per-build per-locale gettext catalogs, which are
    # loose files in the client depot under bin/<build>/res/texts. The dump reads
    # them from there and content-addresses each catalog.
    IDX_FILELIST = REPO_ROOT / "scripts" / ".download_idx.tmp"
    IDX_FILELIST.write_bytes(
        b"regex:\\.idx$\n"
        b"regex:bin[/\\\\]\\d+[/\\\\]res[/\\\\]texts[/\\\\].*[/\\\\]LC_MESSAGES[/\\\\]global\\.mo$\n"
    )
    PKG_FILELIST = REPO_ROOT / "scripts" / ".download_pkgs.tmp"

    for i, (date_str, manifest_id) in enumerate(download_list):
        idx = i + args.start_from
        print(f"\n{'='*60}")
        print(f"[{idx+1}] {date_str} - manifest {manifest_id}")

        manifest_date = parse_date(date_str)
        content_manifest = find_closest_manifest(manifest_date, content_manifests) if manifest_date else None
        if not content_manifest:
            print(f"  No content-depot ({CONTENT_DEPOT}) manifest near {date_str}; maps would be missing. Skipping.")
            continue

        TEMP_DIR.mkdir(parents=True, exist_ok=True)
        builds_before = set(detect_builds(TEMP_DIR))

        # --- Download idx from both depots (client base + content maps) ---
        print(f"  Downloading idx: client depot {CLIENT_DEPOT}...")
        if not run_steamroom(steam_user, CLIENT_DEPOT, manifest_id, TEMP_DIR, IDX_FILELIST, timeout=900):
            continue
        print(f"  Downloading idx: content depot {CONTENT_DEPOT} (manifest {content_manifest})...")
        if not run_steamroom(steam_user, CONTENT_DEPOT, content_manifest, TEMP_DIR, IDX_FILELIST, timeout=900):
            continue

        # The per-locale gettext catalogs (global.mo) live in the localization depot.
        # Older builds keep them ONLY there -- not in the client depot's
        # bin/<build>/res/texts -- so without this fetch their translations are missed
        # and the toolkit can't localize ship/skill/module names. Non-fatal: a build
        # still dumps without it, but we want it whenever the depot has it.
        loc_manifest = find_closest_manifest(manifest_date, localization_manifests) if manifest_date else None
        if loc_manifest:
            print(f"  Downloading translations: localization depot {LOCALIZATION_DEPOT} (manifest {loc_manifest})...")
            run_steamroom(steam_user, LOCALIZATION_DEPOT, loc_manifest, TEMP_DIR, IDX_FILELIST, timeout=900)
        else:
            print(f"  No localization-depot manifest near {date_str}; translations may be missing.")

        # Detect new builds
        all_builds = detect_builds(TEMP_DIR)
        builds = sorted(set(all_builds) - builds_before)
        print(f"  Found builds: {builds} (new out of {len(all_builds)} in temp)")
        if not builds:
            print("  No new builds, skipping")
            continue

        # --- Per build: resolve minimal pkg set, download it, dump ---
        wsl_archive = to_wsl(ARCHIVE_DIR)
        wsl_temp = to_wsl(TEMP_DIR)
        for build in builds:
            if already_complete(build):
                print(f"    Build {build}: already complete ({map_count(build)} maps), skipping")
                continue

            try:
                pkgs = resolve_pkgs(build)
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
            write_pkg_filelist(pkgs, PKG_FILELIST)

            # The required pkgs are split across both depots; each download grabs
            # whatever subset it holds.
            print(f"    Downloading {len(pkgs)} pkg(s) from depots {CLIENT_DEPOT} + {CONTENT_DEPOT}...")
            ok_client = run_steamroom(steam_user, CLIENT_DEPOT, manifest_id, TEMP_DIR, PKG_FILELIST, timeout=3600)
            ok_content = run_steamroom(steam_user, CONTENT_DEPOT, content_manifest, TEMP_DIR, PKG_FILELIST, timeout=3600)
            if not (ok_client and ok_content):
                print(f"    PKG DOWNLOAD FAILED for build {build}")
                continue

            if completing:
                tool = (f"./target/release/wows-data-mgr complete-build "
                        f"--build {build} --game-dir {wsl_temp} --output {wsl_archive} --with-gui")
            else:
                tool = (f"./target/release/wows-data-mgr dump-renderer-data "
                        f"--build {build} --game-dir {wsl_temp} --output {wsl_archive} --force")
            dump_cmd = ["wsl", "bash", "-lc", f"cd {WSL_REPO} && nix develop --command {tool}"]
            print(f"    {'Completing' if completing else 'Dumping'} build {build}...")
            dump_result = subprocess.run(dump_cmd, capture_output=True, timeout=600,
                                         text=True, encoding="utf-8", errors="replace")
            if dump_result.returncode != 0:
                print(f"    DUMP FAILED (exit {dump_result.returncode}):")
                stdout = (dump_result.stdout or "").strip()[:500].encode("ascii", "replace").decode()
                stderr = (dump_result.stderr or "").strip()[:800].encode("ascii", "replace").decode()
                print(f"      stdout: {stdout}")
                print(f"      stderr: {stderr}")
                continue

            stderr = (dump_result.stderr or "").strip()
            if stderr:
                for line in stderr.splitlines():
                    safe = line.strip().encode("ascii", "replace").decode()
                    if safe and "warning: Git tree" not in safe:
                        print(f"      {safe}")

            discovered[build] = {"manifest_id": manifest_id, "date": date_str}
            print(f"    OK")

    # Final cleanup
    if TEMP_DIR.exists():
        shutil.rmtree(TEMP_DIR, ignore_errors=True)

    print(f"\n{'='*60}")
    print(f"DONE! Archived {len(discovered)} new builds.")

    # Write discovered builds for reference
    if discovered:
        out = REPO_ROOT / "game_versions_discovered.toml"
        with open(out, "w") as f:
            f.write("# Discovered builds from bulk archive\n\n")
            for build in sorted(discovered.keys()):
                info = discovered[build]
                f.write(f"[versions.{build}]\n")
                f.write(f'# date = "{info["date"]}"\n')
                f.write(f"client_depot_id = {CLIENT_DEPOT}\n")
                f.write(f'client_manifest_id = "{info["manifest_id"]}"\n\n')
        print(f"Discovered builds written to {out}")


if __name__ == "__main__":
    main()
