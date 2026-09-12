#!/usr/bin/env python3
"""Verify reproducible reference-script dependencies without running their workflows."""
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = ("check_style.py", "test_style_parity.py", "test_style_workflow.py")


class ScriptLocks(unittest.TestCase):
    def test_reference_locks_match_metadata_without_modification(self):
        for name in SCRIPTS:
            with self.subTest(script=name):
                script = ROOT / "scripts" / name
                lock = script.with_suffix(".py.lock")
                before = lock.read_bytes()
                run = subprocess.run(
                    ["uv", "lock", "--check", "--offline", "--script", str(script)],
                    capture_output=True, text=True, timeout=30,
                )
                self.assertEqual(run.returncode, 0, run.stderr)
                self.assertEqual(lock.read_bytes(), before)

    def test_changed_dependency_fails_before_script_execution(self):
        original = ROOT / "scripts/check_style.py"
        metadata = original.read_text().split("# ///", 2)[1]
        with tempfile.TemporaryDirectory(prefix="fern-script-lock-") as temporary:
            script = Path(temporary) / "probe.py"
            lock = script.with_suffix(".py.lock")
            # Removing the dependency needs no registry metadata, even with a cold
            # cache. An upgraded constraint can fail resolution before --locked.
            changed_metadata = metadata.replace('"rich>=13.0",', "")
            self.assertNotEqual(changed_metadata, metadata)
            script.write_text("# ///" + changed_metadata
                              + '# ///\nprint("SCRIPT_EXECUTED")\n')
            before = original.with_suffix(".py.lock").read_bytes()
            lock.write_bytes(before)
            command = ["uv", "run", "--locked", "--offline",
                       "--cache-dir", str(Path(temporary) / "empty-cache"), str(script)]
            run = subprocess.run(
                command,
                env=dict(os.environ, UV_PYTHON_DOWNLOADS="never"),
                capture_output=True, text=True, timeout=30,
            )
            self.assertNotEqual(run.returncode, 0)
            self.assertIn("--locked", run.stderr)
            self.assertNotIn("SCRIPT_EXECUTED", run.stdout)
            self.assertEqual(lock.read_bytes(), before)
            # The exact fixture succeeds when the lock guard is removed, proving
            # the failure above is the lock contract rather than an offline error.
            command.remove("--locked")
            unlocked = subprocess.run(
                command, env=dict(os.environ, UV_PYTHON_DOWNLOADS="never"),
                capture_output=True, text=True, timeout=30,
            )
            self.assertEqual(unlocked.returncode, 0, unlocked.stderr)
            self.assertIn("SCRIPT_EXECUTED", unlocked.stdout)
            self.assertNotEqual(lock.read_bytes(), before)

    def test_maintained_tasks_and_shebangs_enforce_locks(self):
        config = tomllib.loads((ROOT / "mise.toml").read_text())
        for task in config["tasks"].values():
            body = task.get("run", "")
            body = "\n".join(body) if isinstance(body, list) else body
            self.assertNotIn("uv run scripts/", body)
        for name in SCRIPTS:
            self.assertIn("uv run --locked --script", (ROOT / "scripts" / name).read_text().splitlines()[0])


if __name__ == "__main__":
    unittest.main()
