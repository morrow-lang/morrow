#!/usr/bin/env python3
"""The default Rust distribution contains every native component it needs."""
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import package_release as release


EXECUTABLES = ("fern", "fern-c", "fern-qbe", "fern-test-supervisor")
REQUIRED = (*EXECUTABLES, "libfern_runtime.a", "LICENSE", "THIRD_PARTY_NOTICES.md", "fern-package.json")


class DefaultDistribution(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="fern-default-package-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.stage = self.root / "stage"
        self.stage.mkdir()
        for name in REQUIRED:
            path = self.stage / name
            path.write_bytes(b"fixture\n")
            path.chmod(0o755 if name in EXECUTABLES else 0o644)
        (self.stage / "fern-package.json").write_text(
            json.dumps({"format": 1, "compiler": "rust", "backend": "qbe"}) + "\n")

    def test_default_bundle_keeps_all_components_and_explicit_c_reference(self):
        archive, checksum = release.make_bundle(
            self.stage, self.root / "dist", release.BundleSpec("0.1.0", "linux", "arm64"))
        release.verify_archive(archive, checksum)
        with tarfile.open(archive) as stream:
            self.assertEqual({member.name.split("/", 1)[1] for member in stream}, set(REQUIRED))

    def test_each_missing_component_rejects_before_publication(self):
        for name in REQUIRED:
            with self.subTest(name=name):
                path = self.stage / name
                saved = path.read_bytes()
                path.unlink()
                with self.assertRaises((FileNotFoundError, ValueError)):
                    release.make_bundle(self.stage, self.root / "dist",
                                        release.BundleSpec("0.1.0", "linux", "arm64"))
                self.assertFalse((self.root / "dist").exists())
                path.write_bytes(saved)
                path.chmod(0o755 if name in EXECUTABLES else 0o644)

    def test_helpers_must_be_executable_regular_files(self):
        for name in EXECUTABLES:
            with self.subTest(name=name):
                path = self.stage / name
                for mode in (0o644, 0o641):
                    path.chmod(mode)
                    with self.assertRaises((FileNotFoundError, ValueError)):
                        release.ensure_staging_layout(self.stage)
                path.chmod(0o755)
                path.unlink()
                path.symlink_to(self.stage / "LICENSE")
                with self.assertRaises((FileNotFoundError, ValueError)):
                    release.ensure_staging_layout(self.stage)
                path.unlink()
                path.write_bytes(b"fixture\n")
                path.chmod(0o755)

    def test_marker_must_identify_the_default_compiler(self):
        for marker in ("", "{", '{}', '{"format":1,"compiler":"c","backend":"qbe"}',
                       '{"format":true,"compiler":"rust","backend":"qbe"}',
                       '{"format":1.0,"compiler":"rust","backend":"qbe"}'):
            with self.subTest(marker=marker):
                (self.stage / "fern-package.json").write_text(marker)
                with self.assertRaises((FileNotFoundError, ValueError)):
                    release.ensure_staging_layout(self.stage)

    def test_optional_files_obey_the_same_regular_file_and_size_contract(self):
        readme = self.stage / "README.md"
        readme.symlink_to(self.stage / "LICENSE")
        with self.assertRaises((FileNotFoundError, ValueError)):
            release.ensure_staging_layout(self.stage)
        readme.unlink()
        with readme.open("wb") as stream:
            stream.truncate(release.FILE_LIMIT + 1)
        with self.assertRaises((FileNotFoundError, ValueError)):
            release.ensure_staging_layout(self.stage)

    def test_packaging_failure_preserves_the_previous_archive_and_checksum(self):
        spec = release.BundleSpec("0.1.0", "linux", "arm64")
        archive, checksum = release.make_bundle(self.stage, self.root / "dist", spec)
        before = (archive.read_bytes(), checksum.read_bytes())
        with patch.object(tarfile.TarFile, "add", side_effect=OSError("write failed")):
            with self.assertRaises(OSError):
                release.make_bundle(self.stage, self.root / "dist", spec)
        self.assertEqual((archive.read_bytes(), checksum.read_bytes()), before)
        self.assertEqual(set(archive.parent.iterdir()), {archive, checksum})

    def test_archive_helpers_require_owner_execute_permission(self):
        archive = self.root / "non-executable.tar.gz"
        with tarfile.open(archive, "w:gz") as stream:
            for name in REQUIRED:
                entry = tarfile.TarInfo("fern-0.1.0-linux-arm64/" + name)
                entry.mode = 0o641 if name == "fern-qbe" else 0o755
                content = (self.stage / name).read_bytes()
                entry.size = len(content)
                stream.addfile(entry, io.BytesIO(content))
        checksum = self.root / "non-executable.sha256"
        checksum.write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + "  non-executable.tar.gz\n")
        with self.assertRaisesRegex(ValueError, "executable"):
            release.verify_archive(archive, checksum)

    def test_valid_checksum_cannot_hide_a_symlinked_native_helper(self):
        archive = self.root / "forged.tar.gz"
        with tarfile.open(archive, "w:gz") as stream:
            for name in REQUIRED:
                entry = tarfile.TarInfo("fern-0.1.0-linux-arm64/" + name)
                entry.mode = 0o755 if name in EXECUTABLES else 0o644
                if name == "fern-qbe":
                    entry.type = tarfile.SYMTYPE
                    entry.linkname = "/outside/helper"
                    stream.addfile(entry)
                else:
                    content = (self.stage / name).read_bytes()
                    entry.size = len(content)
                    stream.addfile(entry, io.BytesIO(content))
        checksum = self.root / "forged.sha256"
        checksum.write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + "  forged.tar.gz\n")
        with self.assertRaises(ValueError):
            release.verify_archive(archive, checksum)


if __name__ == "__main__":
    unittest.main()
