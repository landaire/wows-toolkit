#!/usr/bin/env python3
"""Ensure the local build environment is ready.

Currently: confirm NASM is available (required for rav1e's `asm` feature,
which powers our CPU AV1 encoder). On Windows we shell out to the
PowerShell installer; elsewhere we just check PATH and tell the user how
to install if missing.
"""

from __future__ import annotations

import platform
import shutil
import subprocess
import sys
from pathlib import Path


def have_nasm() -> bool:
    return shutil.which("nasm") is not None


def install_hint() -> str:
    system = platform.system()
    if system == "Linux":
        return "sudo apt-get install nasm   (or your distro's equivalent)"
    if system == "Darwin":
        return "brew install nasm"
    return "winget install -e --id NASM.NASM"


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent

    if have_nasm():
        print(f"nasm already on PATH: {shutil.which('nasm')}")
        return 0

    if platform.system() == "Windows":
        script = repo_root / "scripts" / "install-nasm-windows.ps1"
        powershell = shutil.which("pwsh") or shutil.which("powershell")
        if powershell is None:
            print("Neither pwsh nor powershell found on PATH.", file=sys.stderr)
            return 1
        cmd = [powershell, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(script)]
        result = subprocess.run(cmd)
        if result.returncode != 0:
            return result.returncode
        if have_nasm():
            print("setup ok")
            return 0
        print(
            "nasm installed into .tooling/ but not on PATH yet. Re-open your shell or run "
            "the project's mise activation again."
        )
        return 0

    print(f"nasm not found. Install with: {install_hint()}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
