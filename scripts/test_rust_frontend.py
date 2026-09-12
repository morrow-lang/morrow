#!/usr/bin/env python3
"""Specification-grounded native comparison of the Rust and C frontends."""
import argparse
import json
import os
from pathlib import Path
import random
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def command(argv, environment, cwd):
    return subprocess.run(list(map(str, argv)), env=environment, cwd=cwd,
                          text=True, capture_output=True, timeout=30)


def verify_case(case, source, rust, reference, environment, directory):
    actual = command([rust, "run", source], environment, directory)
    expected = case["stdout"]
    if actual.returncode != case.get("exit", 0) or actual.stdout != expected:
        raise AssertionError(f"Rust {source.name}: exit={actual.returncode}, stdout={actual.stdout!r}\n{actual.stderr}")
    original = command([reference, "run", source], environment, directory)
    equivalent = original.returncode == actual.returncode and original.stdout == actual.stdout
    if not equivalent and case.get("reference", "match") == "match":
        raise AssertionError(f"Reference mismatch {source.name}: {original.returncode}, {original.stdout!r}\n{original.stderr}")
    return {"case": source.name, "rust": "pass", "reference_matches": equivalent,
            "reference_stdout": original.stdout, "reference_exit": original.returncode}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust-bin", default="compiler-rs/target/debug/fern-rs")
    parser.add_argument("--report")
    args = parser.parse_args()
    rust = (ROOT / args.rust_bin).resolve()
    reference = ROOT / "bin/fern-c"
    environment = dict(os.environ, FERN_QBE=str(ROOT / "bin/fern-qbe"),
                       FERN_RUNTIME_LIB=str(ROOT / "bin/libfern_runtime.a"))
    records = []
    with tempfile.TemporaryDirectory(prefix="fern-rust-differential-") as temp:
        directory = Path(temp)
        manifest = json.loads((ROOT / "compiler-rs/tests/corpus/cases.json").read_text())
        for case in manifest:
            source = ROOT / "compiler-rs/tests/corpus" / case["file"]
            records.append(verify_case(case, source, rust, reference, environment, directory))
        # Bounded positive arithmetic keeps both backends in the shared correct subset.
        rng = random.Random(0xFE12)
        for index in range(16):
            a, b, c = (rng.randrange(1, 50) for _ in range(3))
            source = directory / f"generated-{index}.fn"
            source.write_text(f"fn main():\n    println(({a} + {b}) * {c})\n")
            records.append(verify_case({"stdout": f"{(a+b)*c}\n"}, source, rust, reference, environment, directory))
        invalid = [
            ('fn main():\n    missing\n', "unknown"),
            ('fn main():\n    let n: Int = true\n', "Int"),
            ('fn main():\n    if 1: 2 else: 3\n', "Bool"),
            ('fn main():\n    [1, true]\n', ""),
            ('fn main():\n    9223372036854775808\n', ""),
        ]
        for index, (text, detail) in enumerate(invalid):
            source = directory / f"invalid-{index}.fn"
            output = directory / f"invalid-{index}"
            source.write_text(text)
            actual = command([rust, "build", source, "-o", output], environment, directory)
            if actual.returncode == 0 or output.exists() or "panicked" in actual.stderr or detail.lower() not in actual.stderr.lower():
                raise AssertionError(f"Invalid input not rejected clearly: {text!r}\n{actual.stderr}")
        # Literal shell metacharacters must remain paths at every tool boundary.
        source = directory / "hello 'source' $literal.fn"
        source.write_text('fn main():\n    println("literal paths")\n')
        output = directory / "program 'output' $literal"
        built = command([rust, "build", source, "-o", output], environment, directory)
        if built.returncode or command([output], environment, directory).stdout != "literal paths\n":
            raise AssertionError(built.stdout + built.stderr)
    divergences = [record["case"] for record in records if not record["reference_matches"]]
    report = {"positive_programs": len(records), "invalid_programs": len(invalid),
              "known_reference_divergences": divergences, "cases": records}
    if args.report:
        Path(args.report).write_text(json.dumps(report, indent=2) + "\n")
    print(f"Rust native evaluation passed: {len(records)} programs, {len(invalid)} invalid inputs, literal path handling")
    print(f"Reference differences (Rust matches specification oracle): {', '.join(divergences) or 'none'}")


if __name__ == "__main__":
    main()
