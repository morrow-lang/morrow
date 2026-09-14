# Native optimization, 2026-09-14

These are the list-optimization measurements. The later
[arithmetic follow-up](ARITHMETIC.md) has separate compiler hashes and timings.

The unchanged immutable model is **2.98–3.71× faster** than the previous
GC-frame improvement (`7f53818`). Small known callbacks now inline into bounded
list loops, and Cranelift optimizes the generated native code for speed.

| Whole-process workload | Previous Fern | Current Fern | Optimized Rust |
| --- | ---: | ---: | ---: |
| 10,000 updates × 256 entries | 25.67 ms | **8.61 ms** | 3.41 ms |
| 100,000 updates × 256 entries | 230.47 ms | **62.15 ms** | 12.03 ms |
| 20 million scalar steps | 81.60 ms | 80.41 ms | 57.25 ms |
| Build the Fern workload source | 42.97 ms | 44.67 ms | Not remeasured |

Scalar performance is essentially unchanged. Source builds take about 4% longer
in this experiment. Median peak model RSS changes from **3.42 to 3.48 MiB** at
10,000 updates and **3.44 to 3.52 MiB** at 100,000. The compiled workload is
567,704 bytes, compared with 567,880 before, using the same native runtime archive.
These are macOS executables using system libraries. Rust remains faster: about
2.5× on the shorter model run and 5.2× on the longer one. Startup overhead masks
some of the longer-running difference. Bun was not rerun for this optimization.

## What each change contributed

Each row below is a separate paired run using frozen compiler/program artifacts.
The runtime archive, source, inputs and Rust comparator stay fixed throughout.
Small differences between runs reflect normal desktop measurement variation;
do not multiply these ratios to reconstruct the final comparison above.

| Change isolated in the pair | 10,000 updates, before → after | 100,000 updates, before → after |
| --- | ---: | ---: |
| Enable Cranelift `speed` only | 25.70 → 25.37 ms | 230.78 → 227.14 ms |
| Then use bounded direct list access | 25.27 → 24.10 ms | 226.19 → 213.27 ms |
| Then inline eligible callbacks | 23.92 → 8.88 ms | 213.42 → 62.16 ms |
| With both lowering changes, compare `none` → `speed` | 9.08 → 8.58 ms | 65.74 → 61.35 ms |

Enabling the backend optimizer alone did little for this runtime-call-heavy
workload. Removing callback calls supplied the largest improvement. Once those
calls were gone, backend optimization contributed another 6–7%. These results
support the combined implementation; they do not establish a universal speedup.

1. **Optimized native emission.** Cranelift now uses `opt_level=speed` for native
   object emission, including cross-target builds. Frame pointers and the verifier
   stay enabled. Memory accesses gain no speculative alias/nontrapping flags, and
   no fast-math policy is introduced. This setting applies to generated programs
   independently of whether the Rust compiler itself was built in debug or release.
2. **Bounded list loops.** The compiler validates an input list's length once,
   roots its nonmoving backing storage and emits guarded full-width loads. Map
   and filter populate fresh, preallocated output storage directly, preserving
   its initialized-prefix length across allocating callbacks. General indexing
   retains bounds/fault handling. Actor continuation lowering is unchanged.
3. **Small known callbacks.** Native list callbacks with a statically known
   lifted function and an eligible body of at most 64 typed-expression nodes
   inline into the enclosing function. Parameter/capture counts are also bounded
   at 64. Captures load from the already-evaluated environment; source arguments
   and captures are not reevaluated per element. Allocation and arithmetic faults
   remain supported through the enclosing physical function's roots/fault path.
   Dynamic/larger callbacks and callbacks with calls, nested closures, early
   returns, `Try`, deferred cleanup, loops or actor behavior keep their existing
   invocation path. Ordinary function calls and Option/Result callbacks are not
   newly inlined. Existing 200,000-node emission and 65,536-root-slot limits remain.

Lists still allocate fresh collections and share unchanged managed records.
This introduces neither an in-place language operation nor a new collection
representation. Further work includes allocation/layout costs and broader
inlining; this experiment does not attribute all remaining time or measure
distributed actor throughput.

## Correctness evidence

The new regressions failed on their intended pre-optimization properties before
implementation: repeated arithmetic remained in native objects, bounded list
loops retained per-element runtime calls, and known callbacks retained indirect
calls. They are accompanied by independent observable native oracles:

* Generated ARM64/x86-64 objects eliminate repeated pure arithmetic. Existing
  native ABI tests retain signed integer faults, IEEE payloads and exact widths.
* Bounded map/filter/fold/find/any/all loops avoid repeated list get/append calls;
  arbitrary indexing still uses its checked helper. Native tests cover empty,
  sparse and complete filters, short-circuit searches, mixed payload widths,
  signed zero, retained aliases and precise collection inside callbacks.
* Actual inlined bodies are checked for capture/input evaluation order, Unicode,
  exact integers/floats and forced precise GC before record allocation. Three
  seeded update sequences retain and independently check every historical
  version. Fault tests preserve the original arithmetic fault, caller cleanup,
  reclamation and a subsequent successful call. Dynamic, cleanup/return and
  oversized callbacks have explicit fallback controls.
* Each model comparison verifies 147 independently calculated boundary/seed
  outputs before timing and checks every timed output. The scalar/build harness
  verifies 162 outputs, including scalar and retained-original model results from
  all 12 freshly built executables. Oracles use modular exponentiation and modular
  visit counting rather than translations of the measured loops.

The complete macOS `cargo xtask check` passes **2,193 Rust tests**, 311 native
fixtures, 20 examples, 63 dynamic compatibility programs, 295 atomic rejections
and 64+192+231 fuzz cases, with formatting, notices and Clippy checks.

## Retained measurements and reproduction

Apple M4, 24 GiB RAM, macOS 26.5.1 ARM64; pinned Rust
`1.100.0-nightly (f248f4038 2026-09-05)`. Parent source `7f53818`; final source is
the implementation accompanying this report. Compiler stages use release builds
and the same release runtime archive from the parent. Rust uses the retained
`-O -C strip=symbols -C panic=abort` executable from the original comparison.
No project build/test overlaps timed workload runs. Ordinary desktop applications
remain present; these are not controlled production latency measurements.

Model/scalar timings use nine rotated whole-process rounds after a separately
retained full-workload warmup. Source builds use five rotated rounds plus warmup,
one fixed `FERN_RUNTIME_LIB`, and unique output binaries. Timings include
`/usr/bin/time -l` launch/reaping; source timings include native emission and
system linking. All samples are retained; no timing threshold runs in CI.

* [Final model comparison and compiler-stage hashes](results/codegen-final-macos-arm64-20260914/)
* [Backend-only comparison](results/codegen-speed-macos-arm64-20260914/)
* [Direct-list comparison](results/codegen-lists-macos-arm64-20260914/)
* [Callback-inlining comparison](results/codegen-inline-macos-arm64-20260914/)
* [Backend/lowering interaction](results/codegen-interaction-macos-arm64-20260914/)
* [Scalar/build samples, raw streams and input hashes](results/codegen-scalar-build-macos-arm64-20260914/)

Build the previous (`7f53818`) and current compilers in separate checkouts with
`cargo build --release -p fern`. Use one release-built native runtime archive for
both; compile the unchanged `programs/workloads.fn` into distinct executables.
From the repository root on macOS:

```sh
rustc --edition=2024 -Dwarnings benchmarks/language-comparison/src/immutable.rs -o /tmp/fern-immutable-runner
rustc --edition=2024 -Dwarnings benchmarks/language-comparison/src/codegen.rs -o /tmp/fern-codegen-runner
/tmp/fern-immutable-runner /absolute/before-program /absolute/after-program /absolute/rust-program /tmp/new-model-results
/tmp/fern-codegen-runner /absolute/before-compiler /absolute/after-compiler /absolute/runtime.a /absolute/before-program /absolute/after-program /absolute/rust-program /tmp/new-codegen-results
```

Output directories must not already exist. The codegen harness leaves its 12
fresh binaries under `builds/` for local verification; checked-in evidence retains
their hashes and streams, not those executable artifacts. Individual ablations
use frozen stage binaries documented above. The temporary `none` variant with
both lowering changes is an experiment, not a public compiler option.
