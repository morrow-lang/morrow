#!/usr/bin/env python3
"""Measure existing C/Rust frontend binaries without rebuilding shared artifacts.

This is a bounded local migration experiment, not a portable performance claim.
A successful report requires every command and generated executable to succeed.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


class EvaluationError(RuntimeError):
    """A failed measurement or semantic check invalidates the entire evaluation."""


def fixture(functions: int = 100) -> str:
    """Generate a deterministic common-subset call chain with expected stdout 100."""
    definitions = []
    for index in range(functions):
        value = "x" if index == 0 else f"step{index - 1}(x)"
        definitions.append(f"fn step{index}(x: Int) -> Int:\n    {value} + 1\n")
    definitions.append(f"fn main() -> Int:\n    println(step{functions - 1}(0))\n    0\n")
    return "\n".join(definitions)


def execute(command: list[str], environment: dict[str, str], timeout: float) -> tuple[float, str]:
    """Run literal arguments with a deadline; return milliseconds and stdout on success."""
    start = time.perf_counter_ns()
    try:
        result = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True,
                                text=True, timeout=timeout, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise EvaluationError(f"command {command!r}: {error}") from error
    elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
    if result.returncode != 0:
        details = (result.stdout + result.stderr)[-8000:]
        raise EvaluationError(f"command {command!r} exited {result.returncode}:\n{details}")
    return elapsed_ms, result.stdout


def summarize(samples: list[float]) -> dict:
    """Report raw samples and nearest-rank p95 without implying statistical confidence."""
    ordered = sorted(samples)
    return {"samples_ms": samples, "median_ms": statistics.median(samples),
            "p95_ms": ordered[math.ceil(len(ordered) * 0.95) - 1]}


def artifact(path: Path) -> dict:
    """Record the exact existing artifact's path, byte size, and content digest."""
    try:
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        return {"path": str(path), "bytes": path.stat().st_size, "sha256": digest}
    except OSError as error:
        raise EvaluationError(f"required artifact {path}: {error}") from error


def tool_version(command: list[str], environment: dict[str, str]) -> str:
    """Read optional host metadata with a short timeout, without gating compilation."""
    try:
        _, output = execute(command, environment, 5)
        return output.strip().splitlines()[0] if output.strip() else "unavailable"
    except EvaluationError:
        return "unavailable"



def git_is_dirty(environment: dict[str, str]) -> bool | None:
    """Distinguish an uncommitted evaluation from the recorded HEAD revision."""
    try:
        _, output = execute(["git", "status", "--porcelain", "--untracked-files=normal"], environment, 5)
        return bool(output.strip())
    except EvaluationError:
        return None


def metadata(options: argparse.Namespace, environment: dict[str, str], source: str) -> dict:
    """Describe host, tool versions, exact input, and limitations of this local run."""
    return {
        "utc": datetime.now(timezone.utc).isoformat(),
        "host": {"system": platform.system(), "release": platform.release(),
                 "machine": platform.machine(), "logical_cpus": os.cpu_count()},
        "tools": {"python": platform.python_version(),
                  "rustc": tool_version(["rustc", "--version"], environment),
                  "cargo": tool_version(["cargo", "--version"], environment),
                  "cc": tool_version([environment.get("CC", "cc"), "--version"], environment)},
        "git_commit": tool_version(["git", "rev-parse", "HEAD"], environment),
        "git_dirty": git_is_dirty(environment),
        "configuration": {"frontend_samples": options.samples, "build_samples": options.build_samples,
                          "warmups_per_frontend_action": 1, "timeout_seconds": options.timeout,
                          "order": "C then Rust, alternating for each sample",
                          "source_bytes": len(source.encode()), "functions": 101,
                          "source_sha256": hashlib.sha256(source.encode()).hexdigest(),
                          "expected_stdout": "100\n", "expected_exit": 0,
                          "LIBRARY_PATH_removed": "LIBRARY_PATH" in os.environ,
                          "FERN_QBE": environment["FERN_QBE"],
                          "FERN_RUNTIME_LIB": environment.get("FERN_RUNTIME_LIB", "automatic discovery")},
        "caveats": [
            "Binaries are supplied by the caller; this script neither rebuilds nor verifies release flags.",
            "git_dirty records uncommitted source state; artifact SHA256 values identify the measured binaries.",
            "Process startup, parsing and checking are included in check/emit timings; emit includes stdout capture.",
            "Warm filesystem caches and fixed C/Rust order can affect results; no CPU isolation is applied.",
            "Native build includes frontend, QBE, assembly and linking; it is not a frontend-only measurement.",
            "p95 uses nearest rank; few samples do not support broad latency claims.",
            "One synthetic common-subset fixture cannot establish full-language correctness or performance.",
            "C embeds QBE; Rust's QBE sidecar is reported separately. Shared runtime/system library costs are excluded.",
        ],
    }


def verify_emission(output: str, label: str) -> None:
    """Reject empty or obviously non-IR emit output before accepting its timing."""
    if "function" not in output or not any(entry in output for entry in ["$main(", "$fern_main("]):
        raise EvaluationError(f"{label} emit did not produce a QBE main function")


def measure_frontends(compilers: dict[str, Path], source: Path, options: argparse.Namespace,
                      environment: dict[str, str]) -> dict:
    """Warm and time check/emit in alternating frontend order for each action/sample."""
    timings = {label: {} for label in compilers}
    for action in ["check", "emit"]:
        for label, compiler in compilers.items():
            _, output = execute([str(compiler), action, str(source)], environment, options.timeout)
            if action == "emit":
                verify_emission(output, label)
        samples = {label: [] for label in compilers}
        for _ in range(options.samples):
            for label, compiler in compilers.items():
                elapsed, output = execute([str(compiler), action, str(source)], environment, options.timeout)
                if action == "emit":
                    verify_emission(output, label)
                samples[label].append(elapsed)
        for label in compilers:
            timings[label][action] = summarize(samples[label])
    return timings


def measure_builds(compilers: dict[str, Path], source: Path, options: argparse.Namespace,
                   environment: dict[str, str], timings: dict) -> None:
    """Time native builds and validate stdout/exit for every resulting executable."""
    samples = {label: [] for label in compilers}
    native_sizes = {label: [] for label in compilers}
    for index in range(options.build_samples):
        for label, compiler in compilers.items():
            output = source.parent / f"{label}-{index}"
            elapsed, _ = execute([str(compiler), "build", str(source), "-o", str(output)],
                                 environment, options.timeout)
            _, stdout = execute([str(output)], environment, options.timeout)
            if stdout != "100\n":
                raise EvaluationError(f"{label} executable stdout mismatch: expected '100\\n', got {stdout!r}")
            samples[label].append(elapsed)
            native_sizes[label].append(output.stat().st_size)
    for label in compilers:
        timings[label]["native_build"] = summarize(samples[label])
        timings[label]["native_executable_bytes"] = native_sizes[label]
        timings[label]["native_verified"] = options.build_samples


def evaluate(options: argparse.Namespace) -> dict:
    """Collect a complete report only after all supplied compilers pass verification."""
    environment = os.environ.copy()
    environment.pop("LIBRARY_PATH", None)
    environment["FERN_QBE"] = str(options.qbe)
    compilers = {"c": options.c_compiler, "rust": options.rust_compiler}
    artifacts = {label: artifact(path) for label, path in compilers.items()}
    artifacts["rust_qbe_sidecar"] = artifact(options.qbe)
    source_text = fixture()
    context = metadata(options, environment, source_text)
    with tempfile.TemporaryDirectory(prefix="fern-frontend-evaluation-") as directory:
        source = Path(directory) / "fixture.fn"
        source.write_text(source_text, encoding="utf-8")
        timings = measure_frontends(compilers, source, options, environment)
        measure_builds(compilers, source, options, environment, timings)
    return {"status": "success", "metadata": context, "artifacts": artifacts, "metrics": timings}


def arguments() -> argparse.Namespace:
    """Parse bounded sample counts and caller-selected existing artifacts/output."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--c-compiler", type=Path, default=ROOT / "bin/fern-c")
    parser.add_argument("--rust-compiler", type=Path, default=ROOT / "compiler-rs/target/release/fern-rs")
    parser.add_argument("--qbe", type=Path, default=ROOT / "bin/fern-qbe")
    parser.add_argument("--samples", type=int, default=15)
    parser.add_argument("--build-samples", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=30)
    parser.add_argument("--output", type=Path, required=True)
    options = parser.parse_args()
    if not 1 <= options.samples <= 200 or not 1 <= options.build_samples <= 20:
        parser.error("samples must be 1..200 and build-samples 1..20")
    if not 0 < options.timeout <= 120:
        parser.error("timeout must be greater than 0 and at most 120 seconds")
    for key in ["c_compiler", "rust_compiler", "qbe", "output"]:
        setattr(options, key, getattr(options, key).resolve())
    return options


def main() -> int:
    """Write success or explicit failure JSON, replacing any stale successful report."""
    options = arguments()
    try:
        report = evaluate(options)
    except (EvaluationError, OSError) as error:
        report = {"status": "failed", "error": str(error)}
    try:
        options.output.parent.mkdir(parents=True, exist_ok=True)
        options.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    except OSError as error:
        print(f"cannot write report: {error}", file=sys.stderr)
        return 1
    if report["status"] == "failed":
        print(report["error"], file=sys.stderr)
        return 1
    print(f"Evaluation passed; report: {options.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
