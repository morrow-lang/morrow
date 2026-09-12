#!/usr/bin/env python3
"""Verify SQLite handle lifetimes and exact quotas in debug/release/sanitized builds."""
import argparse
import os
from pathlib import Path
import shlex
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def checked(argv, environment, directory):
    """Run one bounded command and retain the complete failing output."""
    result = subprocess.run(argv, cwd=directory, env=environment, capture_output=True,
                            text=True, timeout=60)
    assert result.returncode == 0, (argv, result.returncode, result.stdout, result.stderr)
    return result


def main():
    """Build isolated SQL source alongside the existing frozen runtime archive."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, default=ROOT / "bin/libfern_runtime.a")
    args = parser.parse_args()
    environment = dict(os.environ, ASAN_OPTIONS="detect_leaks=0",
                       UBSAN_OPTIONS="halt_on_error=1:print_stacktrace=1")
    environment.pop("LIBRARY_PATH", None)
    cflags = shlex.split(checked(["pkg-config", "--cflags", "bdw-gc"], environment, ROOT).stdout)
    libs = shlex.split(checked(["pkg-config", "--libs", "bdw-gc", "sqlite3", "openssl"], environment, ROOT).stdout)
    modes = [("debug", ["-O0", "-g"]), ("release", ["-O2", "-DNDEBUG"]),
             ("sanitized", ["-O1", "-g", "-fsanitize=address,undefined", "-fno-omit-frame-pointer"])]
    with tempfile.TemporaryDirectory(prefix="fern-sql-") as temporary:
        directory = Path(temporary)
        for name, flags in modes:
            binary = directory / name
            source = ROOT / "runtime/fern_sql.c"
            isolated = [source] if source.exists() else []
            checked(["clang", "-std=c11", "-Wall", "-Wextra", "-Wpedantic", "-Werror",
                     "-Iruntime", *flags, *cflags, ROOT / "tests/fixtures/sql_runtime.c",
                     *isolated, args.runtime.resolve(), *libs, "-pthread", "-lm", "-o", binary],
                    environment, ROOT)
            for scenario in ("lifecycle", "capacity", "transaction"):
                result = checked([binary, scenario, directory / (name + scenario)], environment, directory)
                assert result.stdout == f"ok:{scenario}\n", result
                assert not result.stderr, result
            print(f"{name}: 3 SQL lifecycle groups passed")


if __name__ == "__main__":
    main()
