# Compiler phase benchmarks

This separate, unpublished workspace measures the actual Rust frontend with
Criterion 0.5.1. It adds no dependencies to the production compiler. Current
commands use the repository's **nightly-2026-09-06** pin; the numeric Cargo floor
is 1.100, with no stable MSRV promise. Always use `--locked`. The independent
lock follows the current Rust compiler workspace. The Rust 1.75 dependency
pins used in the historical Decision107 measurements no longer apply.
No Cargo install is needed. Historical measurements below are not measurements
of the new nightly; compare compiler changes on the same pinned toolchain.

From the repository root:

```sh
cargo test --manifest-path benchmarks/compiler-phases/Cargo.toml --locked
cargo clippy --manifest-path benchmarks/compiler-phases/Cargo.toml --locked --all-targets -- -D warnings
cargo bench --manifest-path benchmarks/compiler-phases/Cargo.toml --locked --bench phases -- --test
cargo bench --manifest-path benchmarks/compiler-phases/Cargo.toml --locked --bench phases -- --save-baseline local
cargo bench --manifest-path benchmarks/compiler-phases/Cargo.toml --locked --bench phases -- --baseline local
```

Mise integration exposes the smoke run as `rust-bench-smoke` and the statistical
run as `rust-bench`; the explicit `--bench phases` avoids passing Criterion
options to Rust's library-test runner. A populated Cargo cache also supports
`--offline`; first setup must fetch the locked developer dependencies.

The three fixtures are a 64-function scalar program, a generic recursive
component instantiated at two types, and a derived JSON record. Each is parsed,
checked and emitted before timing starts. Tests also verify its REPL behavior,
generic specialization, entry ABI, and actual codec/storage graph. Nine cases
measure parsing source bytes, checking a pre-parsed AST, or emitting already
checked IR into inspection text through the shared lowering module. This does
not measure Cranelift object emission. A tenth validates the actual JSON record codec plan. The proof case
measures validation, not JSON encoding throughput or native runtime execution.

Inputs and returned results cross `std::hint::black_box`; allocation and drop
of the measured phase's output are included. Fixture construction, source I/O,
process startup and preparation of earlier phases are outside the loops.
The source-byte throughput label is a fixture-size normalization, not a claim
that checking/emission scan only that many bytes. No benchmark is an empty
stand-in for a future phase.

Each case uses 50 samples, 500 ms warm-up and a 2 s measurement target. Criterion
stores estimates, confidence intervals and raw sample data in this package's
`target/criterion/`. Its default Rayon/plotting/async features are disabled;
`cargo_bench_support` is sufficient for this command-line workload.
See the [pinned feature manifest](https://github.com/bheisler/criterion.rs/blob/0.5.1/Cargo.toml)
and [Criterion's measurement guide](https://bheisler.github.io/criterion.rs/book/user_guide/timing_loops.html).

`baseline-guidance107.json` records the 2026-09-06 Rust 1.75 development run, fixture
implementation and lock hashes, toolchain, and confidence intervals. It validates
the statistical workflow; it is not a performance threshold or comparison.
Other agents were using the host during measurement. Re-measure before/after
changes on the same quiet machine/toolchain and inspect distributions before
claiming improvement. CI uses the deterministic smoke run, not timing thresholds.

Native correctness, memory/resource and platform acceptance are separate
from these phase measurements; run `cargo xtask check` for those gates.

## Actor preparation review

The 2026-09-06 [paired phase record](actor105-preparation-review.json) compares exact unchanged
fixtures on Rust 1.75/Criterion 0.5.1 in release mode: pre-actor commit 05c4d27, the
frozen actor candidate, and the borrowed-program optimization. Order was
pre-actor/actor/optimized/optimized/actor/pre-actor, giving paired reversals.
It records source/build identities, load, per-run 95% intervals and timing scope.

| Fixture | Pre-actor µs | Actor µs | Optimized µs | Improvement from actor |
| --- | ---: | ---: | ---: | ---: |
| Scalar 64 functions | 184.26 | 273.09 | 204.21 | 25.22% |
| Generic SCC | 42.22 | 58.41 | 45.94 | 21.35% |
| JSON record | 13.46 | 16.96 | 13.95 | 17.71% |

Values average two run medians on macOS arm64. This exploratory shared-host run
removes 77–86% of the added phase time; 3.6–10.8% remains versus pre-actor with all
original universal validation retained. Host contention/frequency was not controlled.
This does not measure end-to-end compilation or native execution, and does not
isolate the cost of each internal pass. Non-actor IL matches pre-actor bytes;
actor IL and fault mappings match the unoptimized actor candidate.
