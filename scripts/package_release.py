#!/usr/bin/env python3
"""Create and verify complete Fern compiler and native-component bundles."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import stat
import tarfile
import tempfile
from dataclasses import dataclass
from pathlib import Path


EXECUTABLES = ("fern", "fern-c", "fern-qbe", "fern-test-supervisor")
REQUIRED_STAGING_FILES = (*EXECUTABLES, "libfern_runtime.a", "LICENSE",
                          "THIRD_PARTY_NOTICES.md", "fern-package.json")
OPTIONAL_STAGING_FILES = ("README.md", "docs/COMPATIBILITY_POLICY.md")
PACKAGE_IDENTITY = {"format": 1, "compiler": "rust", "backend": "qbe"}
FILE_LIMIT = 128 * 1024 * 1024
TOTAL_LIMIT = 512 * 1024 * 1024


@dataclass(frozen=True)
class BundleSpec:
    """Bundle naming metadata."""

    version: str
    os_name: str
    arch: str

    def __post_init__(self) -> None:
        """Keep artifact metadata inside a single, well-formed filename."""
        if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?",
                            self.version):
            raise ValueError("invalid version: expected a semantic version such as 0.1.0")
        for label, value in (("OS", self.os_name), ("architecture", self.arch)):
            if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", value):
                raise ValueError(f"invalid {label}: expected a filename-safe platform name")

    @property
    def stem(self) -> str:
        """Return stable bundle stem."""
        return f"fern-{self.version}-{self.os_name}-{self.arch}"


def fail(message: str) -> int:
    """Print an error and return failure."""
    print(f"ERROR: {message}")
    return 1


def detect_os() -> str:
    """Map host OS into normalized artifact token."""
    system = platform.system().lower()
    if system == "darwin":
        return "macos"
    if system == "linux":
        return "linux"
    return system


def detect_arch() -> str:
    """Map host arch into normalized artifact token."""
    machine = platform.machine().lower()
    aliases = {
        "x86_64": "x86_64",
        "amd64": "x86_64",
        "aarch64": "arm64",
        "arm64": "arm64",
    }
    return aliases.get(machine, machine)


def sha256_file(path: Path) -> str:
    """Compute sha256 for a file."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def ensure_staging_layout(staging: Path) -> None:
    """Ensure required release inputs are present."""
    if not staging.exists() or not staging.is_dir():
        raise FileNotFoundError(f"staging directory not found: {staging}")

    missing: list[str] = []
    total = 0
    for rel in REQUIRED_STAGING_FILES + OPTIONAL_STAGING_FILES:
        path = staging / rel
        try:
            info = path.lstat()
        except FileNotFoundError:
            if rel in REQUIRED_STAGING_FILES:
                missing.append(rel)
            continue
        if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= FILE_LIMIT:
            missing.append(f"{rel} (must be a bounded regular file)")
        if rel in EXECUTABLES and not info.st_mode & 0o100:
            missing.append(f"{rel} (must be executable)")
        total += info.st_size

    if missing:
        joined = ", ".join(missing)
        raise FileNotFoundError(f"staging layout invalid; missing/invalid: {joined}")
    if total > TOTAL_LIMIT:
        raise ValueError("release components exceed aggregate byte limit")
    with (staging / "fern-package.json").open("rb") as stream:
        validate_marker(stream.read(65537))


def validate_marker(data: bytes) -> None:
    """Require the bounded marker identifying this distribution's compiler contract."""
    if len(data) > 65536:
        raise ValueError("package marker exceeds byte limit")
    try:
        identity = json.loads(data)
    except (ValueError, UnicodeError) as error:
        raise ValueError("invalid package marker") from error
    if identity != PACKAGE_IDENTITY or type(identity["format"]) is not int:
        raise ValueError("package marker must identify the Rust compiler and QBE backend")


def make_bundle(staging: Path, out_dir: Path, spec: BundleSpec) -> tuple[Path, Path]:
    """Create tar.gz bundle plus sha256 file."""
    ensure_staging_layout(staging)
    out_dir.mkdir(parents=True, exist_ok=True)

    bundle_root = spec.stem
    archive_path = out_dir / f"{bundle_root}.tar.gz"
    checksum_path = out_dir / f"{bundle_root}.tar.gz.sha256"

    with tempfile.TemporaryDirectory(prefix=".fern-release-", dir=out_dir) as temporary:
        prepared_archive = Path(temporary) / archive_path.name
        prepared_checksum = Path(temporary) / checksum_path.name
        with tarfile.open(prepared_archive, "w:gz") as tar:
            for rel in REQUIRED_STAGING_FILES + OPTIONAL_STAGING_FILES:
                src = staging / rel
                if rel in REQUIRED_STAGING_FILES or src.exists():
                    tar.add(src, arcname=f"{bundle_root}/{rel}", recursive=False)
        digest = sha256_file(prepared_archive)
        prepared_checksum.write_text(f"{digest}  {archive_path.name}\n", encoding="utf-8")
        verify_archive(prepared_archive, prepared_checksum)
        os.replace(prepared_archive, archive_path)
        os.replace(prepared_checksum, checksum_path)
    return archive_path, checksum_path


def verify_archive(archive_path: Path, checksum_path: Path) -> None:
    """Verify archive checksum and required bundle contents."""
    if not archive_path.exists():
        raise FileNotFoundError(f"archive not found: {archive_path}")
    if not checksum_path.exists():
        raise FileNotFoundError(f"checksum not found: {checksum_path}")

    checksum_text = checksum_path.read_text(encoding="utf-8").strip()
    expected_hash = checksum_text.split()[0] if checksum_text else ""
    actual_hash = sha256_file(archive_path)
    if expected_hash != actual_hash:
        raise ValueError("archive checksum mismatch")

    if archive_path.stat().st_size > TOTAL_LIMIT:
        raise ValueError("release archive exceeds byte limit")
    allowed = set(REQUIRED_STAGING_FILES + OPTIONAL_STAGING_FILES)
    seen, root, total = set(), None, 0
    with tarfile.open(archive_path, "r:gz") as tar:
        for member in tar:
            parts = member.name.split("/", 1)
            if root is None:
                root = parts[0]
            if (len(parts) != 2 or parts[0] != root or root in ("", ".", "..")
                    or parts[1] not in allowed or parts[1] in seen):
                raise ValueError("archive contains an unexpected or duplicate member")
            name = parts[1]
            if not member.isfile() or not 0 < member.size <= FILE_LIMIT:
                raise ValueError("archive component must be a bounded regular file")
            if name in EXECUTABLES and not member.mode & 0o100:
                raise ValueError(f"archive component must be executable: {name}")
            total += member.size
            if total > TOTAL_LIMIT:
                raise ValueError("archive exceeds aggregate component byte limit")
            if name == "fern-package.json":
                with tar.extractfile(member) as stream:
                    validate_marker(stream.read(65537))
            seen.add(name)
    missing = set(REQUIRED_STAGING_FILES) - seen
    if missing:
        raise ValueError(f"archive missing required members: {', '.join(sorted(missing))}")


def parse_args() -> argparse.Namespace:
    """Parse CLI arguments."""
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    verify_layout = sub.add_parser("verify-layout", help="Verify staging layout")
    verify_layout.add_argument("--staging", default="bin")

    package = sub.add_parser("package", help="Create release bundle")
    package.add_argument("--staging", default="bin")
    package.add_argument("--out-dir", default="dist")
    package.add_argument("--version", required=True)
    package.add_argument("--os", dest="os_name", default=detect_os())
    package.add_argument("--arch", default=detect_arch())

    verify_archive_cmd = sub.add_parser("verify-archive", help="Verify an existing release bundle")
    verify_archive_cmd.add_argument("--archive", required=True)
    verify_archive_cmd.add_argument("--checksum", required=True)
    return parser.parse_args()


def main() -> int:
    """Program entry point."""
    args = parse_args()

    if args.command == "verify-layout":
        try:
            ensure_staging_layout(Path(args.staging))
        except (FileNotFoundError, ValueError) as exc:
            return fail(str(exc))
        print(f"Staging layout valid: {args.staging}")
        return 0

    if args.command == "package":
        staging = Path(args.staging)
        out_dir = Path(args.out_dir)
        try:
            spec = BundleSpec(version=args.version, os_name=args.os_name, arch=args.arch)
            archive_path, checksum_path = make_bundle(staging, out_dir, spec)
            verify_archive(archive_path, checksum_path)
        except (FileNotFoundError, ValueError, OSError, tarfile.TarError) as exc:
            return fail(str(exc))
        print(f"Created release bundle: {archive_path}")
        print(f"Created checksum: {checksum_path}")
        return 0

    if args.command == "verify-archive":
        try:
            verify_archive(Path(args.archive), Path(args.checksum))
        except (FileNotFoundError, ValueError, OSError, tarfile.TarError) as exc:
            return fail(str(exc))
        print(f"Release bundle verified: {args.archive}")
        return 0

    return fail(f"unknown command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())
