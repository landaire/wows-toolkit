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
    - wows-data-mgr built: cargo build --release -p wows-data-mgr (via WSL nix)
    - Steam credentials (steamroom will prompt interactively on first run)

Steam App 552990 uses multiple depots:
    552993 - Client (~59 GiB): idx, pkg, exe, scripts
    552991 - Content (~20 GiB): additional game assets
    552994 - Localizations (~252 MiB): translation .mo files

Each WoWs update ships TWO builds (current + previous version), so we
skip every other major manifest and dump both builds from one download.

Downloads proceed oldest-to-newest so steamroom can delta-patch
from the previous manifest, saving bandwidth.
"""

import argparse
import shutil
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

TEMP_DIR = Path(r"G:\wows_builds\temp_game_data")
ARCHIVE_DIR = Path(r"G:\wows_builds")
BINARIES_DIR = Path(r"G:\wows_builds\binaries")
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

# Filelist for client depot (depot 552993)
CLIENT_FILELIST = REPO_ROOT / "scripts" / ".download_filelist.tmp"

# Filelist for localization depot (depot 552994) - just .mo files
LOCALIZATION_FILELIST = REPO_ROOT / "scripts" / ".download_translations_only.tmp"


def parse_manifests(path: Path) -> list[tuple[str, str]]:
    """Parse a manifest list file. Returns [(date_str, manifest_id), ...] oldest first."""
    entries = []
    for line in path.read_text(encoding="utf-8").strip().split("\n"):
        parts = line.split("\t")
        if len(parts) != 2:
            continue
        entries.append((parts[0].strip(), parts[1].strip()))
    entries.reverse()  # oldest first
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
    """Check if this build already has a complete dump."""
    for entry in ARCHIVE_DIR.iterdir():
        if entry.is_dir() and entry.name.endswith(f"_{build}") and (entry / "metadata.toml").exists():
            return True
    return False


def run_steamroom(steam_user: str, depot: int, manifest: str, output: Path,
                  filelist: Path, timeout: int = 3600) -> bool:
    """Run steamroom download. Returns True on success."""
    cmd = [
        str(STEAMROOM),
        "--username", steam_user,
        "--remember-password",
        "download",
        "--app", str(APP_ID),
        "--depot", str(depot),
        "--manifest", manifest,
        "--output", str(output),
        "--filelist", str(filelist),
        "--max-downloads", "4",
        "--non-atomic",
    ]
    for attempt in range(2):
        if attempt > 0:
            print(f"  Retry attempt {attempt}...")
            time.sleep(30)
        try:
            result = subprocess.run(cmd, timeout=timeout)
        except subprocess.TimeoutExpired:
            print(f"  DOWNLOAD TIMED OUT")
            continue
        if result.returncode == 0:
            return True
        print(f"  DOWNLOAD FAILED (exit {result.returncode})")
    return False


def copy_translations(temp_dir: Path, archive_dir: Path):
    """Copy translation .mo files from temp download to archive build dirs."""
    bin_dir = temp_dir / "bin"
    if not bin_dir.exists():
        return
    for build_dir in sorted(bin_dir.iterdir()):
        if not build_dir.is_dir() or not build_dir.name.isdigit():
            continue
        build = build_dir.name
        texts_dir = build_dir / "res" / "texts"
        if not texts_dir.exists():
            continue
        # Find the archive dir for this build
        target = None
        for entry in archive_dir.iterdir():
            if entry.is_dir() and entry.name.endswith(f"_{build}") and (entry / "metadata.toml").exists():
                target = entry
                break
        if not target or (target / "translations").exists():
            continue
        trans_dest = target / "translations"
        for lang_dir in texts_dir.iterdir():
            if not lang_dir.is_dir():
                continue
            mo_src = lang_dir / "LC_MESSAGES" / "global.mo"
            if mo_src.exists():
                mo_dest = trans_dest / lang_dir.name / "LC_MESSAGES" / "global.mo"
                mo_dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(mo_src, mo_dest)
        if trans_dest.exists():
            print(f"    Copied translations -> {target.name}")


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

    loc_manifests = parse_manifests(LOCALIZATION_MANIFESTS_FILE) if LOCALIZATION_MANIFESTS_FILE.exists() else []

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

    ARCHIVE_DIR.mkdir(parents=True, exist_ok=True)
    discovered = {}

    # Ensure filelists exist
    if not CLIENT_FILELIST.exists():
        # Written with double backslashes for regex character classes
        CLIENT_FILELIST.write_bytes(
            b"regex:bin[/\\\\]\\d+[/\\\\]idx[/\\\\].*\\.idx$\n"
            b"regex:res_packages[/\\\\].*\\.pkg$\n"
            b"regex:WorldOfWarships.*\\.exe$\n"
            b"regex:scripts\\.zip$\n"
            b"regex:bin[/\\\\]\\d+[/\\\\]res[/\\\\]texts[/\\\\].*[/\\\\]LC_MESSAGES[/\\\\]global\\.mo$\n"
        )

    for i, (date_str, manifest_id) in enumerate(download_list):
        idx = i + args.start_from
        print(f"\n{'='*60}")
        print(f"[{idx+1}] {date_str} - manifest {manifest_id}")

        TEMP_DIR.mkdir(parents=True, exist_ok=True)
        builds_before = set(detect_builds(TEMP_DIR))

        # --- Download client depot (552993) ---
        print(f"  Downloading client depot {CLIENT_DEPOT}...")
        if not run_steamroom(steam_user, CLIENT_DEPOT, manifest_id, TEMP_DIR, CLIENT_FILELIST):
            continue

        # Detect new builds
        all_builds = detect_builds(TEMP_DIR)
        builds = sorted(set(all_builds) - builds_before)
        print(f"  Found builds: {builds} (new out of {len(all_builds)} in temp)")
        if not builds:
            print("  No new builds, skipping")
            continue

        # --- Download localization depot (552994) if manifest exists ---
        manifest_date = parse_date(date_str)
        loc_manifest = find_closest_manifest(manifest_date, loc_manifests) if manifest_date else None
        if loc_manifest and LOCALIZATION_FILELIST.exists():
            print(f"  Downloading localizations depot {LOCALIZATION_DEPOT}...")
            loc_temp = TEMP_DIR / "_loc_temp"
            loc_temp.mkdir(parents=True, exist_ok=True)
            if run_steamroom(steam_user, LOCALIZATION_DEPOT, loc_manifest, loc_temp,
                             LOCALIZATION_FILELIST, timeout=300):
                copy_translations(loc_temp, ARCHIVE_DIR)
            shutil.rmtree(loc_temp, ignore_errors=True)

        # --- Dump each build ---
        for build in builds:
            wsl_archive = str(ARCHIVE_DIR).replace("\\", "/").replace("G:", "/mnt/g")
            wsl_temp = str(TEMP_DIR).replace("\\", "/").replace("G:", "/mnt/g")

            # Register temp dir for wows-data-mgr
            reg_cmd = [
                "wsl", "bash", "-lc",
                f"cd /mnt/g/dev/wows-toolkit && nix develop --command "
                f"./target/release/wows-data-mgr register --latest --path {wsl_temp}"
            ]
            subprocess.run(reg_cmd, capture_output=True, text=True, timeout=120)

            dump_cmd = [
                "wsl", "bash", "-lc",
                f"cd /mnt/g/dev/wows-toolkit && nix develop --command "
                f"./target/release/wows-data-mgr dump-renderer-data "
                f"--build {build} --output {wsl_archive}"
            ]
            print(f"    Dumping build {build}...")
            dump_result = subprocess.run(dump_cmd, capture_output=True, timeout=600,
                                         text=True, encoding="utf-8", errors="replace")
            if dump_result.returncode != 0:
                print(f"    DUMP FAILED (exit {dump_result.returncode}):")
                stdout = (dump_result.stdout or "").strip()[:500].encode("ascii", "replace").decode()
                stderr = (dump_result.stderr or "").strip()[:500].encode("ascii", "replace").decode()
                print(f"      stdout: {stdout}")
                print(f"      stderr: {stderr}")
                continue

            # Print warnings from stderr
            stderr = (dump_result.stderr or "").strip()
            if stderr:
                for line in stderr.splitlines():
                    safe = line.strip().encode("ascii", "replace").decode()
                    if safe and "warning: Git tree" not in safe:
                        print(f"      {safe}")

            discovered[build] = {"manifest_id": manifest_id, "date": date_str}

            # Archive exe and scripts.zip with version-based dir name
            version_str = "unknown"
            for entry in ARCHIVE_DIR.iterdir():
                if entry.is_dir() and entry.name.endswith(f"_{build}") and (entry / "metadata.toml").exists():
                    version_str = entry.name.rsplit(f"_{build}", 1)[0]
                    break

            bin_dir = BINARIES_DIR / f"{version_str}_{build}"
            bin_dir.mkdir(parents=True, exist_ok=True)
            build_dir = TEMP_DIR / "bin" / str(build)

            for exe in list(build_dir.rglob("WorldOfWarships*.exe")) + list(TEMP_DIR.glob("WorldOfWarships*.exe")):
                dest = bin_dir / exe.name
                if not dest.exists():
                    shutil.copy2(exe, dest)
                    print(f"    Archived {exe.name} -> {bin_dir.name}")

            for scripts_zip in list(build_dir.rglob("scripts.zip")) + list(TEMP_DIR.glob("scripts.zip")):
                dest = bin_dir / "scripts.zip"
                if not dest.exists():
                    shutil.copy2(scripts_zip, dest)
                    print(f"    Archived scripts.zip -> {bin_dir.name}")
                break

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
