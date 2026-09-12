#!/usr/bin/env python3
"""Run native Rust stdlib applications with private files and exact output oracles."""
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = {
    "files": ('''fn workflow() -> Result(Int, Int):
    let written = fs.write("data.txt", "Fern 🌿")?
    println(written > 0)
    println(fs.read("data.txt")?)
    println(fs.size("data.txt")?)
    let appended = fs.append("data.txt", "!")?
    println(appended > 0)
    println(fs.read("data.txt")?)
    fs.delete("data.txt")
fn main():
    println(Result.is_ok(workflow()))
    println(fs.exists("data.txt"))
    println(Result.is_err(fs.read("missing.txt")))
''', "true\nFern 🌿\n9\ntrue\nFern 🌿!\ntrue\nfalse\ntrue\n"),
    "strings": ('''fn main():
    let pieces = String.split("a,🌿,c", ",")
    println(List.len(pieces))
    println(String.join(List.reverse(pieces), "/"))
    println(List.head(String.lines("first\\nsecond")))
    println(String.to_upper(String.trim(" fern ")))
    println(Option.unwrap_or(String.index_of("hello fern", "fern"), -1))
    println(Option.is_none(String.index_of("hello", "missing")))
    println(Option.unwrap_or(String.char_at("abc", 1), -1))
    println(Option.is_none(String.char_at("abc", 9)))
    println(Option.unwrap_or(String.char_at("🌿", 0), -1))
    println(Option.is_none(String.char_at("abc", -1)))
    println(Option.unwrap_or(String.index_of("abc", ""), -1))
''', "3\nc/🌿/a\nfirst\nFERN\n6\ntrue\n98\ntrue\n240\ntrue\n0\n"),
    "regex": ('''fn main():
    println(Regex.is_match("a42", "[0-9]+"))
    println(String.join(Regex.find_all("a12b34", "[0-9]+"), ":"))
    println(String.join(Regex.split("a,b,c", "[,]"), "/"))
    println(Regex.replace_all("a12b34", "[0-9]+", "x"))
''', "true\n12:34\na/b/c\naxbx\n"),
    "arguments": ('''fn main():
    println(System.args_count())
    println(System.arg(1))
    println(List.get(System.args(), 2))
''', "3\nliteral ; $ value\n🌿\n"),
    "services": ('''fn database() -> Result(Int, Int):
    let db = sql.open(":memory:")?
    sql.execute(db, "create table example (value integer)")
fn mailbox() -> Result(String, Int):
    let pid = actors.start("test")
    let sent = actors.post(pid, "hello mailbox")?
    println(sent >= 0)
    actors.next(pid)
fn main():
    println(Result.is_ok(database()))
    println(Result.unwrap_or(mailbox(), "failure"))
    println(Result.is_ok(json.parse("[]")))
''', "true\ntrue\nhello mailbox\ntrue\n"),
}


def main():
    compiler = ROOT / "compiler-rs/target/debug/fern-rs"
    environment = dict(os.environ, FERN_QBE=str(ROOT / "bin/fern-qbe"),
                       FERN_RUNTIME_LIB=str(ROOT / "bin/libfern_runtime.a"))
    with tempfile.TemporaryDirectory(prefix="fern-stdlib-") as temporary:
        directory = Path(temporary)
        for name, (source, expected) in CASES.items():
            path = directory / f"{name}.fn"
            path.write_text(source)
            command = [compiler, "run", path]
            if name == "arguments":
                command += ["--", "literal ; $ value", "🌿"]
            result = subprocess.run(command, cwd=directory, env=environment,
                                    capture_output=True, text=True, timeout=30)
            if result.returncode or result.stdout != expected:
                raise AssertionError(f"{name}: {result.returncode}, {result.stdout!r}\n{result.stderr}")
        for name in ("tui_objects", "runtime_tuples", "repl_parity", "regex_optional"):
            source = ROOT / "compiler-rs/tests/corpus" / f"{name}.fn"
            expected = source.with_suffix(".stdout").read_text()
            result = subprocess.run([compiler,"run",source],cwd=directory,env=environment,
                                    capture_output=True,text=True,timeout=30)
            if result.returncode or result.stdout != expected:
                raise AssertionError(f"{name}: {result.returncode}, {result.stdout!r}\n{result.stderr}")
    print(f"Rust stdlib passed: {len(CASES) + 4} native applications")


if __name__ == "__main__":
    main()
