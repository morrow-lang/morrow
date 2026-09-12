#!/usr/bin/env python3
"""Check CI performance budgets for build time, startup latency, and binary size."""

from __future__ import annotations

import argparse
import statistics
import subprocess
import sys
import time
from pathlib import Path

DEFAULT_COMPILE_BUDGET_S = 150.0
DEFAULT_STARTUP_P95_BUDGET_MS = 100.0
DEFAULT_BINARY_BUDGET_BYTES = 1_500_000
DEFAULT_STARTUP_RUNS = 30
DEFAULT_BIN_PATH = Path("bin/fern-c")


def run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    """Run a command and return the subprocess result.

    Args:
        cmd: Command and arguments.

    Returns:
        Completed process.
    """

    return subprocess.run(cmd, capture_output=True, text=True, check=False)


def fail(message: str) -> None:
    """Print an error and exit with failure.

    Args:
        message: User-facing failure message.

    Returns:
        Never.
    """

    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def check_compile_budget(budget_seconds: float) -> float:
    """Build from clean and check release compile time budget.

    Args:
        budget_seconds: Maximum allowed build duration.

    Returns:
        Elapsed compile time in seconds.
    """

    start = time.perf_counter()

    clean = run(["mise", "run", "clean"])
    if clean.returncode != 0:
        fail(f"mise run clean failed:\n{clean.stdout}{clean.stderr}")

    build = run(["mise", "run", "release"])
    if build.returncode != 0:
        fail(f"mise run release failed:\n{build.stdout}{build.stderr}")

    elapsed = time.perf_counter() - start
    print(f"compile_time_seconds={elapsed:.2f} (budget <= {budget_seconds:.2f})")

    if elapsed > budget_seconds:
        fail(f"compile time budget exceeded: {elapsed:.2f}s > {budget_seconds:.2f}s")

    return elapsed


def check_binary_size(bin_path: Path, budget_bytes: int) -> int:
    """Check release binary size budget.

    Args:
        bin_path: Path to compiled binary.
        budget_bytes: Maximum allowed bytes.

    Returns:
        Binary size in bytes.
    """

    if not bin_path.exists():
        fail(f"binary not found at {bin_path}")

    size = bin_path.stat().st_size
    print(f"binary_size_bytes={size} (budget <= {budget_bytes})")

    if size > budget_bytes:
        fail(f"binary size budget exceeded: {size} > {budget_bytes}")

    return size


def measure_startup_ms(bin_path: Path, runs: int) -> tuple[float, float, float]:
    """Measure startup latency for `fern --version`.

    Args:
        bin_path: Binary path.
        runs: Number of measured runs.

    Returns:
        Tuple of (median_ms, p95_ms, max_ms).
    """

    samples: list[float] = []

    warmup = run([str(bin_path), "--version"])
    if warmup.returncode != 0:
        fail(f"warmup failed:\n{warmup.stdout}{warmup.stderr}")

    for _ in range(runs):
        start = time.perf_counter()
        proc = run([str(bin_path), "--version"])
        elapsed_ms = (time.perf_counter() - start) * 1000.0
        if proc.returncode != 0:
            fail(f"startup probe failed:\n{proc.stdout}{proc.stderr}")
        samples.append(elapsed_ms)

    median_ms = statistics.median(samples)
    p95_index = max(0, int(round(0.95 * (len(samples) - 1))))
    p95_ms = sorted(samples)[p95_index]
    max_ms = max(samples)
    return median_ms, p95_ms, max_ms


def check_startup_budget(bin_path: Path, runs: int, p95_budget_ms: float) -> tuple[float, float, float]:
    """Validate startup latency budget using p95.

    Args:
        bin_path: Binary path.
        runs: Number of measured runs.
        p95_budget_ms: Maximum allowed p95 latency.

    Returns:
        Tuple of measured startup stats.
    """

    median_ms, p95_ms, max_ms = measure_startup_ms(bin_path, runs)
    print(
        "startup_ms "
        f"median={median_ms:.2f} p95={p95_ms:.2f} max={max_ms:.2f} "
        f"(p95 budget <= {p95_budget_ms:.2f})"
    )

    if p95_ms > p95_budget_ms:
        fail(f"startup p95 budget exceeded: {p95_ms:.2f}ms > {p95_budget_ms:.2f}ms")

    return median_ms, p95_ms, max_ms


def parse_args() -> argparse.Namespace:
    """Parse CLI arguments.

    Returns:
        Parsed argument namespace.
    """

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-build", action="store_true", help="Skip compile-time check and build step")
    parser.add_argument(
        "--compile-budget-s",
        type=float,
        default=DEFAULT_COMPILE_BUDGET_S,
        help="Compile time budget in seconds",
    )
    parser.add_argument(
        "--startup-p95-budget-ms",
        type=float,
        default=DEFAULT_STARTUP_P95_BUDGET_MS,
        help="Startup latency p95 budget in milliseconds",
    )
    parser.add_argument(
        "--binary-budget-bytes",
        type=int,
        default=DEFAULT_BINARY_BUDGET_BYTES,
        help="Binary size budget in bytes",
    )
    parser.add_argument(
        "--startup-runs",
        type=int,
        default=DEFAULT_STARTUP_RUNS,
        help="Number of startup probes",
    )
    parser.add_argument(
        "--bin-path",
        default=str(DEFAULT_BIN_PATH),
        help="Path to compiled fern binary",
    )
    return parser.parse_args()


def main() -> int:
    """Entry point.

    Returns:
        Process exit code.
    """

    args = parse_args()
    bin_path = Path(args.bin_path)

    if args.startup_runs < 5:
        fail("startup-runs must be >= 5")

    print("Performance budget check")
    if not args.skip_build:
        check_compile_budget(args.compile_budget_s)

    check_binary_size(bin_path, args.binary_budget_bytes)
    check_startup_budget(bin_path, args.startup_runs, args.startup_p95_budget_ms)

    print("All performance budgets passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
