#!/usr/bin/env python3
"""Exercise actual default publication with independent compiler/helper fixtures."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class DefaultSelection(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="fern-default-select-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "bin").mkdir()
        for name in ("fern-c", "fern-qbe", "fern-test-supervisor", "libfern_runtime.a"):
            path = self.root / "bin" / name
            path.write_text("reference " + name)
            path.chmod(0o755)
        (self.root / "tools").mkdir()
        cargo = self.root / "tools/cargo"
        cargo.write_text("""#!/usr/bin/env python3
import json, os, pathlib, re, sys
args = sys.argv[1:]
pathlib.Path(os.environ["CARGO_LOG"]).write_text("\\n".join(args))
if os.environ.get("FAIL_CARGO") == "1": sys.exit(23)
profile = "release" if "--release" in args else "debug"
config = pathlib.Path(".cargo/config.toml")
configured = re.search(r'target\\s*=\\s*"([^" ]+)"', config.read_text()) if config.exists() else None
triple = args[args.index("--target") + 1] if "--target" in args else (configured[1] if configured else "")
target = pathlib.Path(args[args.index("--target-dir") + 1])
artifact = target / triple / profile / "fern-rs"
artifact.parent.mkdir(parents=True, exist_ok=True)
artifact.write_text('#!/bin/sh\\nprintf "Rust compiler fixture\\\\n"\\n')
artifact.chmod(0o755)
if os.environ.get("EMPTY_ARTIFACT") == "1": artifact.write_text("")
if os.environ.get("OMIT_ARTIFACT") != "1":
    event = {"reason":"compiler-artifact", "target":{"name":"fern-rs", "kind":["bin"]}, "executable":str(artifact.resolve())}
    print(json.dumps(event))
    if os.environ.get("DUPLICATE_ARTIFACT") == "1": print(json.dumps(event))
""")
        rustc = self.root / "tools/rustc"
        rustc.write_text("#!/bin/sh\necho 'host: test-native-host'\n")
        rustc.chmod(0o755)
        cargo.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.root / "tools") + os.pathsep + os.environ["PATH"],
                        CARGO_TARGET_DIR=str(self.root / "target with 'spaces' $literal"),
                        CARGO_LOG=str(self.root / "cargo-args"))
        self.env.pop("CARGO_BUILD_TARGET", None)

    def invoke(self, *args):
        return subprocess.run(["bash", str(ROOT / "scripts/tasks/build-default"), *args],
                              cwd=self.root, env=self.env, text=True, capture_output=True, timeout=10)

    def test_debug_and_release_publish_rust_without_overwriting_c_reference(self):
        for mode in ("debug", "release"):
            with self.subTest(mode=mode):
                result = self.invoke(mode)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                args = (self.root / "cargo-args").read_text().splitlines()
                self.assertIn("--locked", args)
                self.assertEqual("--release" in args, mode == "release")
                self.assertEqual((self.root / "bin/fern-c").read_text(), "reference fern-c")
                for name in ("fern", "fern-rs"):
                    executed = subprocess.check_output([self.root / "bin" / name], text=True)
                    self.assertEqual(executed, "Rust compiler fixture\n")
                self.assertEqual(json.loads((self.root / "bin/fern-package.json").read_text()),
                                 {"format": 1, "compiler": "rust", "backend": "qbe"})

    def test_failed_build_preserves_previous_default(self):
        (self.root / "bin/fern").write_text("previous compiler")
        self.env["FAIL_CARGO"] = "1"
        result = self.invoke("debug")
        self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
        self.assertEqual((self.root / "bin/fern").read_text(), "previous compiler")
        self.assertFalse((self.root / "bin/fern-package.json").exists())

    def test_incomplete_helpers_prevent_publication(self):
        (self.root / "bin/fern-qbe").unlink()
        result = self.invoke("debug")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("fern-qbe", result.stderr)
        self.assertFalse((self.root / "bin/fern").exists())

    def test_directory_and_empty_helpers_prevent_publication(self):
        helper = self.root / "bin/fern-qbe"
        helper.unlink()
        helper.mkdir()
        result = self.invoke("debug")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("fern-qbe", result.stderr)
        self.assertFalse((self.root / "bin/fern").exists())
        helper.rmdir()
        helper.touch(mode=0o755)
        result = self.invoke("debug")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("fern-qbe", result.stderr)
        self.assertFalse((self.root / "bin/fern").exists())

    def test_configured_cross_target_never_selects_stale_host_path(self):
        config = self.root / ".cargo/config.toml"
        config.parent.mkdir()
        config.write_text('[build]\ntarget = "configured-cross-target"\n')
        target = Path(self.env["CARGO_TARGET_DIR"])
        for mode in ("debug", "release"):
            stale = target / mode / "fern-rs"
            stale.parent.mkdir(parents=True, exist_ok=True)
            stale.write_text("#!/bin/sh\necho stale compiler\n")
            stale.chmod(0o755)
            result = self.invoke(mode)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(subprocess.check_output([self.root / "bin/fern"], text=True),
                             "Rust compiler fixture\n")
            self.env["CARGO_BUILD_TARGET"] = "environment-cross-target"
            args = (self.root / "cargo-args").read_text().splitlines()
            self.assertIn("--target", args)
            self.assertEqual(args[args.index("--target") + 1], "test-native-host")

    def test_current_invocation_requires_a_nonempty_cargo_artifact(self):
        previous = self.root / "bin/fern"
        previous.write_text("previous compiler")
        for variable in ("OMIT_ARTIFACT", "EMPTY_ARTIFACT", "DUPLICATE_ARTIFACT"):
            with self.subTest(variable=variable):
                self.env[variable] = "1"
                result = self.invoke("debug")
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(previous.read_text(), "previous compiler")
                self.env.pop(variable)

    def test_runtime_archive_must_be_a_regular_nonempty_file(self):
        archive = self.root / "bin/libfern_runtime.a"
        archive.unlink()
        archive.mkdir()
        result = self.invoke("debug")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("libfern_runtime.a", result.stderr)
        self.assertFalse((self.root / "bin/fern").exists())

    def test_directory_publication_destinations_preserve_previous_outputs(self):
        for name in ("fern", "fern-rs", "fern-package.json"):
            for linked in (False, True):
                with self.subTest(name=name, linked=linked):
                    fixture = DefaultSelection()
                    fixture.setUp()
                    try:
                        destination = fixture.root / "bin" / name
                        directory = fixture.root / "outside" if linked else destination
                        directory.mkdir()
                        sentinel = directory / "keep"
                        sentinel.write_text("user data")
                        if linked:
                            destination.symlink_to(directory, target_is_directory=True)
                        previous = fixture.root / "bin/fern"
                        if name != "fern":
                            previous.write_text("previous compiler")
                        result = fixture.invoke("debug")
                        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                        self.assertIn(name, result.stderr)
                        self.assertEqual(list(directory.iterdir()), [sentinel])
                        self.assertEqual(sentinel.read_text(), "user data")
                        self.assertFalse((fixture.root / "cargo-args").exists())
                        if name != "fern":
                            self.assertEqual(previous.read_text(), "previous compiler")
                        if linked:
                            self.assertTrue(destination.is_symlink())
                    finally:
                        fixture.doCleanups()


if __name__ == "__main__":
    unittest.main()
