#!/usr/bin/env python3
"""Compare memory/runtime path baselines for Milestone 7.7 Step D.

This script produces a reproducible report that combines:
1. Current native perf snapshot (compile/startup/binary size).
2. Microbenchmark of ownership op cost:
   - Boehm bridge surface: fern_dup / fern_drop
   - Perceus baseline surface: fern_rc_dup / fern_rc_drop
3. WASM/WasmGC feasibility probes based on local toolchain and repository state.
"""

from __future__ import annotations

import argparse
import re
import shutil
import statistics
import subprocess
import tempfile
import textwrap
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def run(cmd: list[str], *, cwd: Path = ROOT, check: bool = True) -> subprocess.CompletedProcess[str]:
    """Run a subprocess command and return a completed process."""

    proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, check=False)
    if check and proc.returncode != 0:
        message = [
            f"Command failed: {' '.join(cmd)}",
            f"exit={proc.returncode}",
            "--- stdout ---",
            proc.stdout,
            "--- stderr ---",
            proc.stderr,
        ]
        raise RuntimeError("\n".join(message))
    return proc


@dataclass
class PerfSnapshot:
    """Performance budget snapshot values."""

    binary_size_bytes: int
    startup_median_ms: float
    startup_p95_ms: float
    startup_max_ms: float


@dataclass
class RcBenchmark:
    """Ownership operation microbenchmark results."""

    iterations: int
    boehm_ns_per_iter: float
    perceus_ns_per_iter: float

    @property
    def perceus_over_boehm_ratio(self) -> float:
        if self.boehm_ns_per_iter <= 0.0:
            return 0.0
        return self.perceus_ns_per_iter / self.boehm_ns_per_iter


@dataclass
class FeasibilitySnapshot:
    """WASM feasibility probes."""

    fern_has_wasm_target: bool
    qbe_has_wasm_backend: bool
    emcc_available: bool
    clang_wasm_target_ok: bool
    clang_wasmgc_flag_ok: bool


def ensure_release_build() -> None:
    """Build release artifacts used by benchmarks."""

    run(["mise", "run", "release"])


def parse_perf_snapshot(stdout: str) -> PerfSnapshot:
    """Parse scripts/check_perf_budget.py output into structured values."""

    size_match = re.search(r"binary_size_bytes=(\d+)", stdout)
    startup_match = re.search(
        r"startup_ms\s+median=([0-9.]+)\s+p95=([0-9.]+)\s+max=([0-9.]+)",
        stdout,
    )
    if size_match is None or startup_match is None:
        raise RuntimeError("Could not parse performance snapshot output")
    return PerfSnapshot(
        binary_size_bytes=int(size_match.group(1)),
        startup_median_ms=float(startup_match.group(1)),
        startup_p95_ms=float(startup_match.group(2)),
        startup_max_ms=float(startup_match.group(3)),
    )


def collect_perf_snapshot(startup_runs: int) -> PerfSnapshot:
    """Collect startup and size metrics using the existing perf-budget checker."""

    proc = run(
        [
            "python3",
            "scripts/check_perf_budget.py",
            "--bin-path",
            "bin/fern",
            "--binary-budget-bytes",
            "4194304",
            "--skip-build",
            "--startup-runs",
            str(startup_runs),
        ]
    )
    return parse_perf_snapshot(proc.stdout)


def compile_microbenchmark(source_path: Path, output_path: Path) -> None:
    """Compile ownership microbenchmark C source."""

    libs = run(["pkg-config", "--libs", "bdw-gc"], check=False)
    gc_libs = libs.stdout.strip().split() if libs.returncode == 0 else ["-lgc"]
    sqlite_libs_probe = run(["pkg-config", "--libs", "sqlite3"], check=False)
    sqlite_libs = (
        sqlite_libs_probe.stdout.strip().split()
        if sqlite_libs_probe.returncode == 0
        else ["-lsqlite3"]
    )
    openssl_libs_probe = run(["pkg-config", "--libs", "openssl"], check=False)
    openssl_libs = (
        openssl_libs_probe.stdout.strip().split()
        if openssl_libs_probe.returncode == 0
        else ["-lssl", "-lcrypto"]
    )
    cmd = [
        "clang",
        "-O3",
        "-std=c11",
        "-Iruntime",
        str(source_path),
        "bin/libfern_runtime.a",
        "-pthread",
        "-o",
        str(output_path),
        *gc_libs,
        *sqlite_libs,
        *openssl_libs,
    ]
    run(cmd)


def parse_micro_output(stdout: str) -> tuple[float, float]:
    """Parse microbenchmark output for boehm/perceus ns per iteration."""

    boehm_match = re.search(r"boehm_ns_per_iter=([0-9.]+)", stdout)
    perceus_match = re.search(r"perceus_ns_per_iter=([0-9.]+)", stdout)
    if boehm_match is None or perceus_match is None:
        raise RuntimeError("Could not parse ownership microbenchmark output")
    return float(boehm_match.group(1)), float(perceus_match.group(1))


def run_rc_microbenchmark(iterations: int, runs_count: int) -> RcBenchmark:
    """Compile and run ownership microbenchmark, returning median per-iter costs."""

    source_text = textwrap.dedent(
        """
        #include "fern_runtime.h"
        #include <stdint.h>
        #include <stdio.h>
        #include <time.h>

        static uint64_t now_ns(void) {
            struct timespec ts;
            clock_gettime(CLOCK_MONOTONIC, &ts);
            return ((uint64_t)ts.tv_sec * 1000000000ull) + (uint64_t)ts.tv_nsec;
        }

        int fern_main(void) {
            const int64_t iters = __ITERS__;
            if (iters <= 0) return 2;

            volatile void* sink = NULL;

            void* p = fern_alloc(16);
            if (p == NULL) return 3;
            uint64_t start = now_ns();
            for (int64_t i = 0; i < iters; i++) {
                p = fern_dup(p);
                fern_drop(p);
            }
            uint64_t boehm_elapsed = now_ns() - start;
            sink = p;

            void* q = fern_rc_alloc(16, FERN_RC_TYPE_TUPLE);
            if (q == NULL) return 4;
            start = now_ns();
            for (int64_t i = 0; i < iters; i++) {
                q = fern_rc_dup(q);
                fern_rc_drop(q);
            }
            uint64_t perceus_elapsed = now_ns() - start;
            sink = q;

            if (sink == NULL) return 5;

            printf("iters=%lld\\n", (long long)iters);
            printf("boehm_ns_per_iter=%.4f\\n", (double)boehm_elapsed / (double)iters);
            printf("perceus_ns_per_iter=%.4f\\n", (double)perceus_elapsed / (double)iters);
            return 0;
        }
        """
    ).strip().replace("__ITERS__", str(iterations))

    boehm_samples: list[float] = []
    perceus_samples: list[float] = []

    with tempfile.TemporaryDirectory(prefix="fern-mem-compare-") as tmp:
        tmpdir = Path(tmp)
        c_path = tmpdir / "ownership_bench.c"
        exe_path = tmpdir / "ownership_bench"
        c_path.write_text(source_text, encoding="utf-8")
        compile_microbenchmark(c_path, exe_path)

        for _ in range(runs_count):
            proc = run([str(exe_path)])
            boehm_ns, perceus_ns = parse_micro_output(proc.stdout)
            boehm_samples.append(boehm_ns)
            perceus_samples.append(perceus_ns)

    return RcBenchmark(
        iterations=iterations,
        boehm_ns_per_iter=statistics.median(boehm_samples),
        perceus_ns_per_iter=statistics.median(perceus_samples),
    )


def probe_wasm_feasibility() -> FeasibilitySnapshot:
    """Probe local toolchain/repo state for wasm bring-up feasibility."""

    help_text = run(["./bin/fern", "--help"]).stdout
    fern_has_wasm_target = (" wasm" in help_text) or ("\n  wasm" in help_text)

    qbe_has_wasm_backend = any(
        (ROOT / "deps" / "qbe" / name).exists()
        for name in ("wasm", "wasm32", "wasm64")
    )

    emcc_available = shutil.which("emcc") is not None

    with tempfile.TemporaryDirectory(prefix="fern-wasm-probe-") as tmp:
        tmpdir = Path(tmp)
        src = tmpdir / "probe.c"
        src.write_text("int main(void) { return 0; }\n", encoding="utf-8")

        clang_wasm_target_ok = (
            run(
                [
                    "clang",
                    "--target=wasm32-unknown-unknown",
                    "-c",
                    str(src),
                    "-o",
                    str(tmpdir / "probe.o"),
                ],
                check=False,
            ).returncode
            == 0
        )

        clang_wasmgc_flag_ok = (
            run(
                [
                    "clang",
                    "--target=wasm32-unknown-unknown",
                    "-mreference-types",
                    "-mwasm-gc",
                    "-c",
                    str(src),
                    "-o",
                    str(tmpdir / "probe_gc.o"),
                ],
                check=False,
            ).returncode
            == 0
        )

    return FeasibilitySnapshot(
        fern_has_wasm_target=fern_has_wasm_target,
        qbe_has_wasm_backend=qbe_has_wasm_backend,
        emcc_available=emcc_available,
        clang_wasm_target_ok=clang_wasm_target_ok,
        clang_wasmgc_flag_ok=clang_wasmgc_flag_ok,
    )


def render_decision(perf: PerfSnapshot, rc: RcBenchmark, feas: FeasibilitySnapshot) -> str:
    """Render a markdown decision artifact."""

    default_path = "Perceus baseline (compiler-inserted dup/drop + RC headers) for first WASM target"
    fallback_path = "Boehm bridge for early bring-up only (non-default, explicitly temporary)"
    wasmgc_status = "defer for first shipping target"

    go_blockers = []
    if not feas.fern_has_wasm_target:
        go_blockers.append("Fern CLI has no WASM compilation target yet")
    if not feas.qbe_has_wasm_backend:
        go_blockers.append("Embedded QBE backend has no wasm target directory in-tree")
    if not feas.clang_wasm_target_ok:
        go_blockers.append("local clang wasm32 object probe failed")

    blocker_text = (
        "\n".join(f"- {item}" for item in go_blockers)
        if go_blockers
        else "- none (toolchain probes passed, still requires compiler/runtime integration work)"
    )

    lines = [
        "# Milestone 7.7 Step D Memory Path Comparison (2026-02-06)",
        "",
        "## Scope",
        "Compare Boehm bridge vs Perceus baseline vs WasmGC feasibility, then choose default + fallback for first WASM target.",
        "",
        "## Measurements (this repository, local run)",
        "",
        "### Native perf snapshot",
        f"- `bin/fern` size: `{perf.binary_size_bytes}` bytes",
        f"- startup median: `{perf.startup_median_ms:.2f}` ms",
        f"- startup p95: `{perf.startup_p95_ms:.2f}` ms",
        f"- startup max: `{perf.startup_max_ms:.2f}` ms",
        "",
        "### Ownership operation microbenchmark",
        f"- iterations per run: `{rc.iterations}`",
        f"- Boehm bridge surface (`fern_dup` + `fern_drop`) median cost: `{rc.boehm_ns_per_iter:.4f}` ns/iter",
        f"- Perceus baseline surface (`fern_rc_dup` + `fern_rc_drop`) median cost: `{rc.perceus_ns_per_iter:.4f}` ns/iter",
        f"- Perceus/Boehm op-cost ratio: `{rc.perceus_over_boehm_ratio:.2f}x`",
        "",
        "Interpretation:",
        "- Boehm bridge dup/drop is effectively near-zero semantic overhead (no-op ownership bridge under GC).",
        "- Perceus baseline ops already incur measurable metadata update cost, acceptable for baseline bring-up and expected to improve once borrow/reuse analysis reduces inserted operations.",
        "",
        "### WASM/WasmGC feasibility probes",
        f"- Fern CLI exposes WASM target: `{feas.fern_has_wasm_target}`",
        f"- Embedded QBE has wasm backend in-tree: `{feas.qbe_has_wasm_backend}`",
        f"- `emcc` available: `{feas.emcc_available}`",
        f"- local clang wasm32 object probe: `{feas.clang_wasm_target_ok}`",
        f"- local clang WasmGC flag probe (`-mwasm-gc`): `{feas.clang_wasmgc_flag_ok}`",
        "",
        "Current blockers for shipping any WASM memory path from this repo state:",
        blocker_text,
        "",
        "## Decision (Step D)",
        f"- default path: **{default_path}**",
        f"- fallback path: **{fallback_path}**",
        f"- WasmGC posture: **{wasmgc_status}**",
        "",
        "## Go/No-Go criteria for entering WASM implementation",
        "1. Add a compiler-level WASM target surface in Fern CLI/codegen.",
        "2. Add/choose a wasm backend path (QBE extension or alternative backend path).",
        "3. Re-run this comparison including real WASM artifact metrics:",
        "   - wasm binary size,",
        "   - startup latency in runtime host,",
        "   - representative runtime microbenchmarks with dup/drop-heavy workloads.",
        "4. Keep Boehm bridge limited to bring-up/testing; do not make it default once Perceus path is production-ready.",
    ]
    return "\n".join(lines) + "\n"


def parse_args() -> argparse.Namespace:
    """Parse command-line arguments."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--iterations",
        type=int,
        default=5_000_000,
        help="Iterations per ownership microbenchmark run",
    )
    parser.add_argument(
        "--runs",
        type=int,
        default=5,
        help="Number of microbenchmark runs (median reported)",
    )
    parser.add_argument(
        "--startup-runs",
        type=int,
        default=20,
        help="Startup probes for perf snapshot",
    )
    parser.add_argument(
        "--output",
        default="docs/reports/memory-path-comparison-2026-02-06.md",
        help="Output markdown report path",
    )
    parser.add_argument(
        "--skip-release-build",
        action="store_true",
        help="Skip `mise run release` before probing",
    )
    return parser.parse_args()


def main() -> int:
    """Entry point."""

    args = parse_args()
    out_path = ROOT / args.output
    out_path.parent.mkdir(parents=True, exist_ok=True)

    if not args.skip_release_build:
        ensure_release_build()

    perf = collect_perf_snapshot(args.startup_runs)
    rc = run_rc_microbenchmark(args.iterations, args.runs)
    feas = probe_wasm_feasibility()

    report = render_decision(perf, rc, feas)
    out_path.write_text(report, encoding="utf-8")

    print(f"Wrote report: {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
