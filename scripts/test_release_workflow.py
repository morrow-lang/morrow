#!/usr/bin/env python3
"""Verify actual release recipe naming and reject unsafe artifact metadata."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class ReleaseWorkflow(unittest.TestCase):
    def test_recipe_builds_one_correctly_named_archive(self):
        with tempfile.TemporaryDirectory(prefix="fern-release-") as temp:
            root = Path(temp)
            for name in ("mise.toml", "LICENSE", "THIRD_PARTY_NOTICES.md", "README.md", "include/version.h",
                         "docs/COMPATIBILITY_POLICY.md", "scripts/package_release.py",
                         "bin/fern", "bin/fern-c", "bin/fern-qbe", "bin/fern-test-supervisor",
                         "bin/fern-package.json", "bin/libfern_runtime.a"):
                destination = root / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(ROOT / name, destination)
            result = subprocess.run(["mise", "run", "--skip-deps", "release-package"], cwd=root,
                                    text=True, capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            version = subprocess.check_output([str(ROOT / "bin/fern"), "--version"],
                                              text=True).strip().split()[-1]
            archives = list((root / "dist").glob("*.tar.gz"))
            self.assertEqual(len(archives), 1)
            self.assertTrue(archives[0].name.startswith("fern-" + version + "-"), archives[0].name)
            self.assertNotRegex(archives[0].name, r"\s")

    def test_invalid_version_is_an_actionable_error(self):
        with tempfile.TemporaryDirectory(prefix="fern-release-invalid-") as temp:
            for version in ("0.1.0\nfern ", "../escape", "0.1.0/escape"):
                result = subprocess.run(["python3", str(ROOT / "scripts/package_release.py"),
                                         "package", "--version", version,
                                         "--staging", str(ROOT / "dist/staging"),
                                         "--out-dir", temp], text=True, capture_output=True, timeout=10)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("invalid version", result.stdout)
                self.assertNotIn("Traceback", result.stderr)
                self.assertEqual(list(Path(temp).iterdir()), [])


if __name__ == "__main__":
    unittest.main()
