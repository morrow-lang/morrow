"""Exercise installed compiler workflows from outside the checkout."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]


class InstallationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="fern-install-")
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.bundle = self.directory / "Fern's tools $bundle"
        self.bundle.mkdir()
        for name in ("fern", "fern-c", "fern-qbe", "fern-test-supervisor",
                     "libfern_runtime.a", "fern-package.json"):
            shutil.copy2(ROOT / "bin" / name, self.bundle / name)
        self.source = self.directory / "hello world.fn"
        self.source.write_text('fn main():\n    println("Hello, Fern!")\n')
        self.env = dict(os.environ, PATH=str(self.bundle) + os.pathsep + os.environ["PATH"])
        self.env.pop("LIBRARY_PATH", None)
        for name in ("FERN_QBE", "FERN_RUNTIME_LIB", "FERN_TEST_SUPERVISOR"):
            self.env.pop(name, None)

    def run_fern(self, executable, *args):
        return subprocess.run([str(executable), *map(str, args)], cwd=self.directory,
                              env=self.env, text=True, capture_output=True, timeout=30)

    def assert_success(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_bundle_on_path_builds_output_with_shell_characters(self):
        output = self.directory / "my 'program' $output"
        self.assert_success(self.run_fern("fern", "build", self.source, "-o", output))
        result = self.run_fern(output)
        self.assert_success(result)
        self.assertEqual(result.stdout, "Hello, Fern!\n")

    def test_default_is_the_rust_compiler_outside_the_checkout(self):
        self.source.write_text(
            'newtype Id = Id(Int)\nfn main():\n'
            '    println(Id(9223372036854775807).0)\n')
        result = self.run_fern("fern", "run", self.source)
        self.assert_success(result)
        self.assertEqual(result.stdout, "9223372036854775807\n")

    def test_default_bundle_runs_native_source_tests(self):
        self.source.write_text('fn test_answer()->Result((),Int):\n'
                               '    if 6 * 7 == 42: Ok(())\n    else: Err(1)\n')
        result = self.run_fern("fern", "test", self.source)
        self.assert_success(result)
        self.assertIn("1/1 passed", result.stdout)

    def test_incomplete_installation_cannot_borrow_checkout_helpers(self):
        for name, command in (("fern-qbe", "run"), ("libfern_runtime.a", "run"),
                              ("fern-test-supervisor", "test")):
            with self.subTest(component=name):
                self.source.write_text('fn main(): println("ok")\nfn test_ok(): ()\n')
                component = self.bundle / name
                saved = self.bundle / (name + ".saved")
                component.rename(saved)
                try:
                    result = self.run_fern("fern", command, self.source)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(name, result.stderr)
                    self.assertIn("installed package", result.stderr)
                finally:
                    saved.rename(component)

    def test_leading_dash_output_is_a_filename(self):
        self.assert_success(self.run_fern("fern", "build", self.source, "-o", "-program"))
        result = self.run_fern(self.directory / "-program")
        self.assert_success(result)
        self.assertEqual(result.stdout, "Hello, Fern!\n")

    def test_symlinked_compiler_finds_bundle_runtime(self):
        link = self.directory / "fern-link"
        link.symlink_to(self.bundle / "fern")
        self.assert_success(self.run_fern(link, "run", self.source))

    def test_run_uses_private_temporary_paths(self):
        # The old basename-based path must never be overwritten or removed.
        name = "fern-protected-" + self.directory.name
        source = self.directory / (name + ".fn")
        source.write_text(self.source.read_text())
        protected = Path("/tmp") / ("fern_" + name)
        protected.write_text("user-owned sentinel")
        self.addCleanup(lambda: protected.unlink(missing_ok=True))
        self.assert_success(self.run_fern(self.bundle / "fern", "run", source))
        self.assertTrue(protected.exists(), "fern run deleted an unrelated file")
        self.assertEqual(protected.read_text(), "user-owned sentinel")

    def test_install_and_uninstall_custom_prefix(self):
        prefix = self.directory / "installed 'tools' $literal"
        env = dict(self.env, PREFIX=str(prefix))
        # Inspect the literal environment-based command; actual argv checks below pin the destination.
        dry = subprocess.run(["mise", "run", "--dry-run", "--skip-deps", "install"], cwd=ROOT,
                             env=env, text=True, capture_output=True, timeout=10)
        self.assertEqual(dry.returncode, 0, dry.stdout + dry.stderr)
        recipe = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]["install"]["run"]
        self.assertIn("${DESTDIR-}${PREFIX-/usr/local}/bin", recipe)
        subprocess.run(["mise", "run", "--skip-deps", "install"], cwd=ROOT, env=env,
                       check=True, capture_output=True, timeout=10)
        self.assertTrue((prefix / "bin/libfern_runtime.a").is_file())
        for name in ("fern-c", "fern-qbe", "fern-test-supervisor", "fern-package.json"):
            self.assertTrue((prefix / "bin" / name).is_file(), name)
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
            self.assertEqual((prefix / "share/fern" / name).read_bytes(),
                             (ROOT / name).read_bytes())
        self.assert_success(self.run_fern(prefix / "bin/fern", "run", self.source))
        subprocess.run(["mise", "run", "uninstall"], cwd=ROOT, env=env,
                       check=True, capture_output=True, timeout=10)
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
            self.assertFalse((prefix / "share/fern" / name).exists(), name)
        self.assertFalse((prefix / "bin/fern").exists())
        self.assertFalse((prefix / "bin/libfern_runtime.a").exists())
        for name in ("fern-c", "fern-qbe", "fern-test-supervisor", "fern-package.json"):
            self.assertFalse((prefix / "bin" / name).exists(), name)

    def test_install_rejects_directory_destinations_before_copying_any_component(self):
        names = ("bin/fern", "bin/fern-c", "bin/fern-qbe", "bin/fern-test-supervisor",
                 "bin/fern-package.json", "bin/libfern_runtime.a",
                 "share/fern/LICENSE", "share/fern/THIRD_PARTY_NOTICES.md")
        for index, name in enumerate(names):
            for linked in (False, True):
                with self.subTest(destination=name, linked=linked):
                    prefix = self.directory / f"blocked-{index}-{linked}"
                    destination = prefix / name
                    destination.parent.mkdir(parents=True)
                    directory = self.directory / f"outside-{index}" if linked else destination
                    directory.mkdir()
                    sentinel = directory / "keep"
                    sentinel.write_text("user data")
                    if linked:
                        destination.symlink_to(directory, target_is_directory=True)
                    previous = prefix / "bin/fern"
                    if name != "bin/fern":
                        previous.parent.mkdir(parents=True, exist_ok=True)
                        previous.write_text("previous compiler")
                    result = subprocess.run(["mise", "run", "--skip-deps", "install"], cwd=ROOT,
                                            env=dict(self.env, PREFIX=str(prefix), DESTDIR=""),
                                            text=True, capture_output=True, timeout=10)
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertIn(name, result.stdout + result.stderr)
                    self.assertEqual(list(directory.iterdir()), [sentinel])
                    self.assertEqual(sentinel.read_text(), "user data")
                    if name != "bin/fern":
                        self.assertEqual(previous.read_bytes(), b"previous compiler")
                    if name != "bin/fern-package.json":
                        self.assertFalse((prefix / "bin/fern-package.json").exists())
                    if linked:
                        self.assertTrue(destination.is_symlink())


if __name__ == "__main__":
    unittest.main()
