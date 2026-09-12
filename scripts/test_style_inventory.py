#!/usr/bin/env python3
"""Directory inventories detect lookup shadowing with literal names and bounded traversal."""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def inventory(helper, roots):
    """Supply bounded line data, never command fragments or shell expansions."""
    return subprocess.run([helper, "tree"], input=b"".join(os.fsencode(path) + b"\n" for path in roots),
                          capture_output=True, timeout=5)


def cases(helper, root):
    """Pin entry types, followed symlinks, missing paths, cycles and input limits."""
    directory = root / "names '🌿 \\ space"
    directory.mkdir()
    (directory / "a #$:\\").write_text("one")
    nested = directory / "nested"
    nested.mkdir()
    (nested / "entry").touch()
    first = inventory(helper, [directory])
    assert first.returncode == 0 and os.fsencode(directory / "a #$:\\") in first.stdout, first
    (directory / "a #$:\\").write_text("two")
    assert inventory(helper, [directory]).stdout == first.stdout
    (directory / "new").touch()
    assert inventory(helper, [directory]).stdout != first.stdout
    outside = root / "outside"
    outside.mkdir()
    link = directory / "link"
    link.symlink_to(outside, target_is_directory=True)
    before = inventory(helper, [directory])
    assert before.returncode == 0 and b"L " in before.stdout
    (outside / "new header.h").touch()
    assert inventory(helper, [directory]).stdout != before.stdout
    missing = inventory(helper, [root / "absent"])
    assert missing.returncode == 0 and missing.stdout.startswith(b"M ")
    (nested / "cycle").symlink_to(directory, target_is_directory=True)
    cycle = inventory(helper, [directory])
    assert cycle.returncode == 0 and b"C " in cycle.stdout
    (directory / "after cycle").touch()
    assert inventory(helper, [directory]).stdout != cycle.stdout
    assert inventory(helper, [root / "absent"] * 129).returncode == 125
    assert inventory(helper, [Path("relative")]).returncode == 125
    assert inventory(helper, [directory / "a #$:\\"]).returncode == 125
    print("10 directory inventory checks passed")


def inaccessible_cases(helper, root):
    """Denied search cannot hide resolvable headers; gaining access changes the inventory."""
    private = root / "private"
    private.mkdir()
    (private / "hidden.h").write_text("hidden")
    link = root / "private-link"
    link.symlink_to(private, target_is_directory=True)
    try:
        private.chmod(0o000)
        assert not os.access(private, os.X_OK), "permission regression requires an unprivileged user"
        denied = inventory(helper, [link])
        assert denied.returncode == 0 and b"U " + os.fsencode(link) + b"\n" in denied.stdout, denied
        assert b"hidden.h" not in denied.stdout
        private.chmod(0o700)
        opened = inventory(helper, [link])
        assert opened.returncode == 0 and b"hidden.h" in opened.stdout, opened
        assert opened.stdout != denied.stdout
        private.chmod(0o100)
        assert os.access(private, os.X_OK) and not os.access(private, os.R_OK)
        unknown = inventory(helper, [link])
        assert unknown.returncode == 125 and b"directory open failed" in unknown.stderr, unknown
    finally:
        private.chmod(0o700)
    print("three inaccessible-directory lookup checks passed")


def main():
    """Exercise the standalone decoder in debug/release and sanitizers without shared artifacts."""
    env = dict(os.environ, ASAN_OPTIONS="detect_leaks=0", UBSAN_OPTIONS="halt_on_error=1")
    env.pop("LIBRARY_PATH", None)
    os.environ.update(ASAN_OPTIONS=env["ASAN_OPTIONS"], UBSAN_OPTIONS=env["UBSAN_OPTIONS"])
    with tempfile.TemporaryDirectory(prefix="fern-style-inventory-") as temporary:
        root = Path(temporary)
        for name, flags in (("debug", ["-O0", "-g"]), ("release", ["-O2", "-DNDEBUG"]),
                            ("sanitized", ["-O1", "-g", "-fsanitize=address,undefined"])):
            helper = root / name
            subprocess.run(["clang", "-std=c11", "-Wall", "-Wextra", "-Werror", *flags,
                            ROOT / "scripts/bootstrap/style_metadata.c", "-o", helper], check=True, env=env)
            target = root / (name + "-cases")
            target.mkdir()
            cases(helper, target)
            inaccessible_cases(helper, target)


if __name__ == "__main__":
    main()
