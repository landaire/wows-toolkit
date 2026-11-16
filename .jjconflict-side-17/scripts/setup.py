#!/usr/bin/env python3
"""Provision the tools a native build needs, without Nix or WSL.

Two tools are not part of a stock Rust install and are not discoverable from
PATH on a fresh Windows machine:

  nasm   rav1e's `asm` feature builds the AV1 SIMD kernels with it.
  clang  ring hardcodes clang when targeting wasm32, and cc-rs has no MSVC
         fallback for that target, so `cargo check -p wt-web --target
         wasm32-unknown-unknown` cannot run without it.

Both are pinned by toolchains/windows/toolchain-manifest.json, which is also
what the Buck toolchain provisions from, so a Cargo build and a Buck build use
the same versions.

Anything discovered here is written to .tooling/dev.env, which mise loads.
"""

from __future__ import annotations

import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "toolchains" / "windows" / "toolchain-manifest.json"
TOOLING = REPO_ROOT / ".tooling"
ENV_FILE = TOOLING / "dev.env"


def manifest_archive(name: str) -> dict:
    archives = json.loads(MANIFEST.read_text(encoding="utf-8"))["archives"]
    for archive in archives:
        if archive["name"] == name:
            return archive
    raise SystemExit(f"{MANIFEST} has no archive named {name!r}.")


def download_verified(url: str, sha256: str, dest: Path) -> None:
    print(f"Downloading {url}")
    digest = hashlib.sha256()
    with urllib.request.urlopen(url) as response, dest.open("wb") as out:
        while chunk := response.read(1 << 20):
            digest.update(chunk)
            out.write(chunk)
    actual = digest.hexdigest()
    if actual != sha256:
        dest.unlink(missing_ok=True)
        raise SystemExit(f"{url}\n  expected sha256 {sha256}\n  got      sha256 {actual}")


def ensure_nasm_windows() -> Path | None:
    """Install the manifest-pinned NASM into .tooling/nasm."""
    existing = shutil.which("nasm")
    if existing:
        print(f"nasm already on PATH: {existing}")
        return Path(existing).parent

    archive = manifest_archive("nasm")
    install_root = TOOLING / "nasm"
    nasm_exe = install_root / "nasm.exe"
    if nasm_exe.exists():
        print(f"nasm already installed at {install_root}")
        return install_root

    install_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        zip_path = Path(tmp) / "nasm.zip"
        download_verified(archive["url"], archive["sha256"], zip_path)
        with zipfile.ZipFile(zip_path) as zf:
            zf.extractall(Path(tmp) / "x")
        # The archive nests everything under nasm-<version>/.
        roots = [p for p in (Path(tmp) / "x").iterdir() if p.is_dir()]
        if len(roots) != 1:
            raise SystemExit(f"Unexpected NASM archive layout: {[p.name for p in roots]}")
        for item in roots[0].iterdir():
            target = install_root / item.name
            if target.exists():
                if target.is_dir():
                    shutil.rmtree(target)
                else:
                    target.unlink()
            shutil.move(str(item), str(target))

    if not nasm_exe.exists():
        raise SystemExit("nasm.exe missing after extraction.")
    print(f"nasm {archive['version']} installed at {install_root}")
    return install_root


def find_vs_clang() -> tuple[Path, Path] | None:
    """Locate clang.exe and llvm-ar.exe from the Visual Studio Clang component."""
    vswhere = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"))
    vswhere = vswhere / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    roots: list[Path] = []
    if vswhere.exists():
        result = subprocess.run(
            [str(vswhere), "-products", "*", "-latest", "-property", "installationPath"],
            capture_output=True,
            text=True,
        )
        roots += [Path(line) for line in result.stdout.splitlines() if line.strip()]

    for root in roots:
        bin_dir = root / "VC" / "Tools" / "Llvm" / "x64" / "bin"
        clang, llvm_ar = bin_dir / "clang.exe", bin_dir / "llvm-ar.exe"
        if clang.exists() and llvm_ar.exists():
            return clang, llvm_ar
    return None


def find_clang_windows() -> tuple[Path, Path] | None:
    found = find_vs_clang()
    if found:
        return found
    # A standalone LLVM install works just as well.
    clang = shutil.which("clang")
    llvm_ar = shutil.which("llvm-ar")
    if clang and llvm_ar:
        return Path(clang), Path(llvm_ar)
    return None


def clang_hint() -> str:
    return (
        "clang and llvm-ar were not found.\n"
        "  ring hardcodes clang for wasm32, so `mise run fix-all` cannot check\n"
        "  the wt-web WASM target without it. Install either:\n"
        "\n"
        "    the Visual Studio component (preferred, matches CI):\n"
        '      "C:\\Program Files (x86)\\Microsoft Visual Studio\\Installer\\setup.exe" modify \\\n'
        "        --add Microsoft.VisualStudio.Component.VC.Llvm.Clang\n"
        "    or standalone LLVM:\n"
        "      winget install -e --id LLVM.LLVM\n"
        "\n"
        "  Then re-run `mise run setup`."
    )


def write_env(entries: dict[str, str]) -> None:
    TOOLING.mkdir(parents=True, exist_ok=True)
    body = "".join(f"{key}={value}\n" for key, value in sorted(entries.items()))
    ENV_FILE.write_text(body, encoding="utf-8", newline="\n")
    print(f"Wrote {ENV_FILE.relative_to(REPO_ROOT)}:")
    for line in body.splitlines():
        print(f"  {line}")


def main() -> int:
    system = platform.system()
    entries: dict[str, str] = {}

    if system != "Windows":
        missing = [tool for tool in ("nasm", "clang", "llvm-ar") if not shutil.which(tool)]
        if missing:
            hint = {
                "Linux": "sudo apt-get install nasm clang llvm   (or your distro's equivalent)",
                # Homebrew's llvm is keg-only, so installing it is not enough:
                # nothing lands on PATH without the second line.
                "Darwin": (
                    'brew install nasm llvm'
                    ' && export PATH="$(brew --prefix llvm)/bin:$PATH"'
                ),
            }.get(system, "install nasm and clang from your package manager")
            print(f"Missing: {', '.join(missing)}. Install with: {hint}", file=sys.stderr)
            return 1
        print("nasm, clang and llvm-ar are all on PATH.")
        write_env({})
        return 0

    ensure_nasm_windows()

    clang = find_clang_windows()
    if clang is None:
        print(clang_hint(), file=sys.stderr)
        return 1
    clang_exe, llvm_ar_exe = clang
    print(f"clang: {clang_exe}")
    print(f"llvm-ar: {llvm_ar_exe}")
    # cc-rs reads these when building for wasm32; without them it looks for a
    # bare "clang" on PATH, which a stock MSVC machine does not have.
    entries["CC_wasm32_unknown_unknown"] = str(clang_exe)
    entries["AR_wasm32_unknown_unknown"] = str(llvm_ar_exe)

    write_env(entries)
    print("setup ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
