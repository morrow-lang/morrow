#!/usr/bin/env python3
"""Check regex capture numbering and absent/empty groups in the shared runtime."""
from pathlib import Path
import shlex
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = (
    ("b", "(a)?(b)", "3\n0:1:b\n-1:-1:\n0:1:b\n"),
    ("a", "(a)(b)?", "3\n0:1:a\n0:1:a\n-1:-1:\n"),
    ("b", "(a*)(b)", "3\n0:1:b\n0:0:\n0:1:b\n"),
    ("c", "((a)(b))?c", "4\n0:1:c\n-1:-1:\n-1:-1:\n-1:-1:\n"),
    ("abc", "(x)", "0\n"),
    ("abc", "[", "0\n"),
    ("abc", "[a-z]+", "1\n0:3:abc\n"),
)


def main():
    flags = subprocess.check_output(
        ["pkg-config", "--libs", "bdw-gc", "sqlite3", "openssl"], text=True,
    )
    with tempfile.TemporaryDirectory(prefix="fern-regex-") as temporary:
        binary = Path(temporary) / "captures"
        subprocess.run(
            ["cc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-Iruntime",
             "tests/fixtures/regex_captures.c", "bin/libfern_runtime.a",
             *shlex.split(flags), "-pthread", "-o", binary], cwd=ROOT, check=True,
        )
        for text, pattern, output in CASES:
            result = subprocess.run([binary, text, pattern], capture_output=True,
                                    text=True, timeout=10)
            assert (result.returncode, result.stdout, result.stderr) == (0, output, ""), (
                text, pattern, result,
            )
    print(f"Regex capture contracts passed: {len(CASES)} cases")


if __name__ == "__main__":
    main()
