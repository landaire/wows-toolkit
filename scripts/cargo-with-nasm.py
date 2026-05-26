#!/usr/bin/env python3
"""Cargo wrapper that ensures NASM is on PATH before invoking cargo.

Useful when rav1e's `asm` feature is in play: rav1e's build.rs needs
nasm available at compile time, and NASM's various install routes
(winget, our .tooling installer, distro packages) put nasm in different
places. This script normalizes that.

Lookup order:
  1. nasm already on PATH                       -> use as-is
  2. <repo>/.tooling/nasm (our mise-run setup)  -> prepend to PATH
  3. C:\\Program Files\\NASM (winget default)     -> prepend to PATH

Falls through to cargo even if nasm isn't found - cargo will produce
the real error for features that need it.
"""

from __future__ import annotations

import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path


def ensure_nasm_on_path(repo_root: Path) -> None:
    if shutil.which("nasm") is not None:
        return

    candidates: list[Path] = [repo_root / ".tooling" / "nasm"]
    if platform.system() == "Windows":
        candidates.append(Path(r"C:\Program Files\NASM"))

    for cand in candidates:
        if (cand / ("nasm.exe" if platform.system() == "Windows" else "nasm")).is_file():
            os.environ["PATH"] = f"{cand}{os.pathsep}{os.environ['PATH']}"
            return


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    ensure_nasm_on_path(repo_root)
    return subprocess.run(["cargo", *sys.argv[1:]]).returncode


if __name__ == "__main__":
    sys.exit(main())
