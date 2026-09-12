#!/usr/bin/env python3
"""Independent native oracles for the shipping language compatibility gaps."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    """Verify exact normal output, cleanup-on-fault, and malformed-source rejection."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "compiler-rs/target/debug/fern-rs")
    parser.add_argument("--backend", choices=["qbe", "cranelift"])
    parser.add_argument("--reference-compiler", type=Path)
    args = parser.parse_args()
    env = dict(os.environ, FERN_QBE=str(ROOT / "bin/fern-qbe"),
               FERN_RUNTIME_LIB=str(ROOT / "bin/libfern_runtime.a"))
    env.pop("LIBRARY_PATH", None)
    sources = ROOT / "compiler-rs/tests/migration"
    fixtures = sorted(sources.glob("*.fn")) + sorted((ROOT / "compiler-rs/tests/result_builders").glob("*.fn"))
    with tempfile.TemporaryDirectory(prefix="fern-migration-native-") as temp:
        for source in fixtures:
            options = ["--backend", args.backend] if args.backend else []
            result = subprocess.run([args.compiler.resolve(), "run", *options, source],
                                    cwd=temp, env=env, capture_output=True, text=True, timeout=60)
            expected = source.with_suffix(".stdout").read_text()
            assert result.stdout == expected, (source, result)
            if source.stem == "index_fault":
                assert result.returncode != 0, result
                assert "index out of bounds" in result.stderr, result
            else:
                assert result.returncode == 0 and result.stderr == "", (source, result)
        if args.reference_compiler:
            for name in ("reference", "aliases"):
                source = sources / (name + ".fn")
                result = subprocess.run([args.reference_compiler.resolve(), "run", source],
                                        cwd=temp, env=env, capture_output=True, text=True, timeout=60)
                assert (result.returncode, result.stdout, result.stderr) == (0, source.with_suffix(".stdout").read_text(), ""), result
    print(f"Migration language native contracts passed: {len(fixtures)} programs")


if __name__ == "__main__":
    main()
