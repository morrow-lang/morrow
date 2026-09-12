#!/usr/bin/env python3
"""Directory success/error contracts through the C ABI and both source frontends."""
from pathlib import Path
import os
import argparse
import shlex
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE = '''fn main():
    match fs.list_dir(System.arg(1)):
        Ok(entries) -> println(List.len(entries))
        Err(code) -> println(code + 100)
'''


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--c-only",action="store_true")
    options=parser.parse_args()
    compilers=[ROOT/"bin/fern-c"]
    if not options.c_only: compilers.append(ROOT/"compiler-rs/target/debug/fern-rs")
    flags = subprocess.check_output(["pkg-config", "--libs", "bdw-gc", "sqlite3", "openssl"], text=True)
    environment = dict(os.environ, FERN_QBE=str(ROOT / "bin/fern-qbe"),
                       FERN_RUNTIME_LIB=str(ROOT / "bin/libfern_runtime.a"))
    with tempfile.TemporaryDirectory(prefix="fern-directory-") as temporary:
        directory = Path(temporary)
        native = directory / "directory-runtime"
        subprocess.run(["cc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-Iruntime",
                        "tests/fixtures/directory_runtime.c", "bin/libfern_runtime.a",
                        *shlex.split(flags), "-pthread", "-o", native], cwd=ROOT, check=True)
        empty = directory / "empty"
        empty.mkdir()
        populated = directory / "populated"
        populated.mkdir()
        for name in ["plain.txt", "🌿.txt"]:
            (populated / name).write_text("contents")
        file = directory / "file"
        file.write_text("ordinary file")
        cases = [(empty, ["ok:0"], "0\n"),
                 (populated, ["ok:2", "plain.txt", "🌿.txt"], "2\n"),
                 (directory / "missing", ["err:1"], "101\n"),
                 (file, ["err:5"], "105\n")]
        source = directory / "listing.fn"
        source.write_text(SOURCE)
        binaries=[]
        for index,compiler in enumerate(compilers):
            binary=directory/f"listing-{index}"
            built=subprocess.run([compiler,"build",source,"-o",binary],env=environment,
                                 capture_output=True,text=True,timeout=30)
            assert built.returncode==0, f"{compiler}: {built.stderr}"
            binaries.append(binary)
        for path, expected_native, expected_source in cases:
            result = subprocess.run([native, path], capture_output=True, text=True, timeout=10)
            assert result.returncode == 0, result.stderr
            assert sorted(result.stdout.splitlines()) == sorted(expected_native), result.stdout
            for compiler,binary in zip(compilers,binaries):
                result = subprocess.run([binary, path], env=environment,
                                        capture_output=True, text=True, timeout=30)
                assert result.returncode == 0, f"{compiler}: {result.stderr}"
                assert result.stdout == expected_source, (compiler, result.stdout)
        source.write_text((ROOT / "tests/fixtures/result_pattern_bindings.fn").read_text())
        expected = (ROOT / "tests/fixtures/result_pattern_bindings.stdout").read_text()
        for compiler in compilers:
            result = subprocess.run([compiler, "run", source], env=environment,
                                    capture_output=True, text=True, timeout=30)
            assert result.returncode == 0, f"{compiler}: {result.stderr}"
            assert result.stdout == expected, (compiler, result.stdout, expected)
        source.write_text(SOURCE.replace("fs.list_dir", "File.list_dir"))
        for compiler in compilers:
            result = subprocess.run([compiler, "check", source], capture_output=True, text=True)
            assert result.returncode == 0, result.stderr
    print(f"Directory Result contracts passed: 4 C ABI cases, {4*len(compilers)} native source programs, {len(compilers)} Result binder regressions, {len(compilers)} alias checks")


if __name__ == "__main__":
    main()
