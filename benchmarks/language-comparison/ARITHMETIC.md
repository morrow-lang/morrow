> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

# Native integer arithmetic optimization

Measured 2026-09-14 on Apple M4 / 24 GiB / macOS 26.5.1 ARM64. The baseline
compiler is the implementation shipped at `dd2f5da` (compiler bytes unchanged
since `48ca830`). Both candidates use the same frozen optimized Rust runtime
archive, Cranelift `opt_level=speed`, benchmark source and runtime arguments.

The unchanged 20-million-step integer recurrence improves from **76.60 ms to
66.57 ms**: **13.1% less elapsed time, or a 1.15× speedup**. Optimized Rust
measures **57.35 ms** in the same rotated run. Fern's gap to Rust falls from
about **34% to 16%** on this workload.

| Paired measurement | Before | After | Rust |
| --- | ---: | ---: | ---: |
| Scalar recurrence, 20 million steps | 76.60 ms | 66.57 ms | 57.35 ms |
| Build the small benchmark source | 43.00 ms | 42.48 ms | Not remeasured |
| 10,000 immutable model updates | 8.30 ms | 8.37 ms | 3.37 ms |
| 100,000 immutable model updates | 59.44 ms | 60.31 ms | 11.54 ms |

These are whole-process medians, including launch, output, shutdown and the
macOS `time -l` wrapper. Scalar uses nine measured rotated rounds after one
retained warmup; builds use five measured rounds and unique output paths. Model
measurements use a separate nine-round paired run. The approximately 1% model
difference and small build difference do not establish a meaningful improvement.
The complete workload executable changes from **567,704 to 567,656 bytes**.
Maximum measured scalar process RSS is **1.5 MiB** before and after.

## Why both changes matter

The original native loop calls a general division/remainder helper for every
`% 2147483647`. This hides the constant divisor from the backend and adds a call,
domain checks and a fault-slot check on each iteration. Direct self-tail calls
also reload and store their parameters through private stack slots.

Two complementary changes address that:

1. When the lowered integer divisor is a known nonzero literal other than `-1`,
   emit a direct native division/remainder operation. For every signed64 dividend,
   that operation cannot divide by zero or overflow. The pinned Cranelift backend
   already implements exact constant-divisor strength reduction; Fern does not
   introduce a custom reciprocal algorithm. Operands are evaluated in the same
   source order before this decision. Zero, `-1` and nonliteral divisors retain
   the existing fault-aware, wrapping helper.
2. Eligible direct self-tail functions whose parameters are all `Int` carry
   parameters through typed SSA phi values instead of private stack slots.
   Every new argument finishes before the simultaneous parameter update. Phi
   backedges use the successful post-fault-check predecessor and are completed
   before entry allocations are inserted. Mixed/managed parameter types keep
   their existing storage path; existing capture and owned-defer exclusions remain.

Disassembly confirms that the resulting scalar loop keeps its counter and value
in registers, with **no loads, stores, calls or division instructions in the
loop**. It uses multiplication, shifts and additions to calculate the exact
remainder. Ordinary floating-point operations and the separate WASM backend are
unchanged.

### Retained experiments, including the unsuccessful first attempt

| Experiment | Scalar before | Scalar after | Finding |
| --- | ---: | ---: | --- |
| Original → constant-divisor change only | 75.47 ms | 85.38 ms | Slower while loop state still travels through memory |
| SSA loop state only → both changes | 75.74 ms | 66.24 ms | Constant arithmetic helps once state stays in registers |
| Original → both changes | 76.60 ms | 66.57 ms | Final paired improvement |

Each row is its own rotated paired run. Do not combine timings from separate
runs into a synthetic speedup. The first attempt is preserved, not omitted
because it was slower. It is not the shipped implementation.

Rust still generates a shorter loop: it hoists constants and uses shifted
add/subtract forms where Cranelift materializes constants in the loop and uses
multiply-add/subtract forms. That identifies remaining code-generation work;
it does not establish that all arithmetic is now within 16% of Rust. Dynamic
division, floating-point loops, vectorization and actor throughput were not
measured by this experiment. Elixir/Bun were not rerun, and the previous
[BEAM comparison](BEAM.md) remains historical evidence for its original compiler.

## Correctness and acceptance

The unchanged recurrence computes `(value * 48271) % 2147483647` with runtime
step count and seed. For 20 million steps and seed 7, an independent modular
exponentiation oracle gives **468592691**. The model oracle counts cell visits
mathematically and checks that the original model remains zero.

New regression coverage includes:

- A failing-before/passing-after lowering check that safe literal division stays
  visible to native optimization; exceptional and nonliteral divisors retain
  the helper path.
- Failing-before/passing-after checks that integer self-tail parameters use phi
  edges without per-iteration parameter-slot traffic, including multiple backedges.
  Existing evaluation-order assertions now check each phi's corresponding
  completed argument and retain the intermediate fault-check requirement.
- **113,508 native quotient/remainder checks**, using independent `i128` arithmetic
  over 18 divisors, signed boundaries and three deterministic random seeds.
  These include full-width values, negative divisors, powers of two and wrapped
  `MIN / -1` and `MIN % -1`.
- **357 independently checked recurrence cases**, including full-width parameter
  permutations, several branches, early returns and 100,000 iterations without
  recursive stack growth. Non-tail self-calls keep their ordinary call semantics.
- Native operand side effects, callback payloads, initial immutable aliases,
  literal/dynamic division-by-zero, faults in later tail arguments, deferred
  cleanup, body-local managed values across precise collection and final heap
  reclamation. The three tail semantic tests passed before and after SSA lowering.

The timed experiments retain **246 samples**: three scalar/build runs of 42
samples each and two model runs of 60 samples each, including tagged warmups.
Every timed scalar/model output is checked; each timed build produces a distinct
executable whose scalar and immutable-model outputs are independently verified.
No unrelated project build/test ran during measurement. Normal desktop apps
remained open; this is one machine without CPU isolation or confidence intervals.

An independent audit recomputed all 246 timing rows and five summary tables,
checked all 90 retained scalar outputs against modular exponentiation, and
matched all 126 scalar/build RSS observations to their unmodified stderr.
Model stdout is checked during measurement but is not retained by that older
harness. The rebuilt shipping compiler matches the measured final compiler
byte-for-byte (SHA-256 `b6edf97ad7850c32ab5e8fd3af15f19ad68339254887233d99ba8841abcaa7c8`).

The final macOS `cargo xtask check` passed formatting, notices, Clippy,
**2,202 Rust tests across 281 suites**, **311 native fixtures**, **20 examples**,
**63 dynamic compatibility programs**, **295 atomic rejections**, and
**64+192+231 fuzz cases**. This includes the production actor simulations and
real three-process cluster stress/recovery tests.

## Reproduce and inspect

Use the existing standalone Rust [codegen harness](src/codegen.rs) and
[immutable harness](src/immutable.rs). Their sources, independent oracles,
compiler/workload/runtime hashes, timings, raw scalar/build streams and build
artifact hashes are recorded with the results. Run from the repository root on
macOS with the pinned Rust toolchain; build the compiler in release mode and
pass the same runtime archive to both compilers. The measured Fern source is
still `programs/workloads.fn`; the optimized Rust comparator is unchanged.

```sh
rustc --edition=2024 -Dwarnings benchmarks/language-comparison/src/codegen.rs -o /tmp/fern-codegen-runner
/tmp/fern-codegen-runner BEFORE_COMPILER AFTER_COMPILER FIXED_RUNTIME \
  BEFORE_WORKLOAD AFTER_WORKLOAD RUST_WORKLOAD NEW_OUTPUT_DIRECTORY
rustc --edition=2024 -Dwarnings benchmarks/language-comparison/src/immutable.rs -o /tmp/fern-model-runner
/tmp/fern-model-runner BEFORE_WORKLOAD AFTER_WORKLOAD RUST_WORKLOAD ANOTHER_NEW_OUTPUT_DIRECTORY
```

Evidence directories:

- [Final scalar and build comparison](results/arithmetic-final-macos-arm64-20260914/)
- [Final immutable-model control](results/arithmetic-final-model-macos-arm64-20260914/)
- [Constant-divisor-only experiment](results/arithmetic-constant-macos-arm64-20260914/)
- [Constant-divisor-only model control](results/arithmetic-constant-model-macos-arm64-20260914/)
- [SSA-only versus combined experiment](results/arithmetic-interaction-macos-arm64-20260914/)

Build outputs remain local; the repository retains their hashes and measurements.
