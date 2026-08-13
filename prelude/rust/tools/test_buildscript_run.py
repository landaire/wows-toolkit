import unittest
from pathlib import Path
import subprocess
import sys
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent))
import buildscript_run


class BuildscriptEnvironmentTest(unittest.TestCase):
    def test_excludes_ambient_environment_values(self) -> None:
        self.assertTrue(hasattr(buildscript_run, "buildscript_environment"))

        environment = buildscript_run.buildscript_environment(
            {
                "OUT_DIR": "/declared/out",
                "CARGO": "/bin/false",
                "CARGO_HOME": "/hostile/cargo-home",
                "HOME": "/hostile/home",
                "PATH": "/hostile/path",
                "RUSTC": "/declared/rustc",
                "SCCACHE_DIR": "/hostile/sccache",
            },
            {"CARGO", "OUT_DIR", "RUSTC"},
            "/buck-buildscript-path-is-disabled",
        )

        self.assertEqual(
            environment,
            {
                "CARGO": "/bin/false",
                "OUT_DIR": "/declared/out",
                "PATH": "/buck-buildscript-path-is-disabled",
                "RUSTC": "/declared/rustc",
            },
        )

    def test_path_cannot_resolve_host_tools(self) -> None:
        environment = buildscript_run.buildscript_environment(
            {"PATH": "/hostile/path"},
            set(),
            "/buck-buildscript-path-is-disabled",
        )

        command = subprocess.run(
            ["/bin/sh", "-c", "command -v sh"],
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertNotEqual(command.returncode, 0)
        self.assertEqual(command.stdout, "")

    def test_rustc_probe_uses_the_allowlisted_environment(self) -> None:
        environment = {
            "HOST": "aarch64-apple-darwin",
            "PATH": "/declared/path",
            "RUSTC": "/declared/rustc",
            "TARGET": "aarch64-apple-darwin",
        }

        with patch("buildscript_run.subprocess.check_output") as check_output:
            buildscript_run.ensure_rustc_available(
                env=environment,
                cwd=Path("/declared/cwd"),
                target="aarch64-apple-darwin",
            )

        check_output.assert_called_once_with(
            ["/declared/rustc", "--version"],
            cwd=Path("/declared/cwd"),
            env=environment,
            shell=False,
        )
