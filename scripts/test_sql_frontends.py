#!/usr/bin/env python3
"""Require identical SQL lifecycle output through the C and Rust native frontends."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    """Compile and execute a fixed source oracle with literal paths and a deadline."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, action="append")
    parser.add_argument("--backend", choices=["qbe", "cranelift"])
    args = parser.parse_args()
    compilers = args.compiler or [ROOT / "bin/fern-c", ROOT / "compiler-rs/target/debug/fern-rs"]
    env = dict(os.environ, FERN_QBE=str(ROOT / "bin/fern-qbe"),
               FERN_RUNTIME_LIB=str(ROOT / "bin/libfern_runtime.a"))
    env.pop("LIBRARY_PATH", None)
    source = ROOT / "compiler-rs/tests/corpus/sql_lifecycle.fn"
    expected = source.with_suffix(".stdout").read_text()
    with tempfile.TemporaryDirectory(prefix="fern-sql-source-") as temporary:
        for compiler in compilers:
            options = ["--backend", args.backend] if args.backend else []
            result = subprocess.run([compiler.resolve(), "run", *options, source],
                                    cwd=temporary, env=env, capture_output=True, text=True, timeout=60)
            assert (result.returncode, result.stdout, result.stderr) == (0, expected, ""), result
    print(f"SQL lifecycle source contracts passed: {len(compilers)} frontend(s)")


if __name__ == "__main__":
    main()
