> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

> Historical document, retained for architectural context. Its implementation paths, commands and measurements describe the retired setup. See [the current build guide](../../BUILD.md) and [roadmap](../../ROADMAP.md).

# Rust frontend evaluation — 2026-09-05

This report records the initial scalar prototype. See
[migration progress](RUST_MIGRATION.md) for the subsequent collections and
error-value milestone; the measurements below remain specific to the original artifacts.

## Recommendation

Continue with Rust for the next frontend milestone, while keeping C as the
shipping compiler until feature and tooling parity are demonstrated. This
experiment establishes an independent safe Rust parser, checker, and typed-IR
emitter that produces native executables through the existing QBE and C runtime.
It does not establish that a complete Rust compiler will be smaller or faster
than the complete C implementation. Odin was not implemented or benchmarked.

The useful result is architectural: resolved semantic types survive every compiler
stage, so string, boolean, and integer operations no longer depend on guessing a
type from register width or source spelling. A process adapter preserves the
existing native backend without introducing unsafe Rust FFI.

## Scope and native evidence

The [prototype guide](../../compiler-rs/README.md) lists the exact supported subset,
limits, build commands, architecture, and remaining language/tooling gaps.
Rust implements `check`, `emit`, `build`, and `run`; normal Fern commands and
release packaging still use C.

Local verification passed `mise run check` (542 C tests and existing integration/style
gates), `mise run rust-check`, `mise run docs-check`, and strict style checks for the C QBE
adapter. Linux/macOS Rust validation is added to CI; Linux execution was not
available in this local macOS run.

The suite includes 51 Rust tests and 32 native programs (16 fixed fixtures and
16 seeded generated programs). Five malformed programs must fail without creating
executables. CLI regressions protect source/output aliases, existing output on
backend failure, temporary cleanup, quoted dependency flags, and literal paths.
Parser tests exercise malformed source prefixes, UTF-8 boundaries, seeded input,
size/token/depth limits, and source diagnostics. Four evaluation-harness tests
ensure failed compilers cannot yield a successful measurement report.

Native tests require exact stdout and exit status against explicit expected
results. Five fixtures expose differences in the existing C backend:

| Fixture | C observation | Rust expectation |
| --- | --- | --- |
| `bool_binding.fn` | Bound boolean printed as `1` | `true` |
| `int64.fn` | Large integers truncated to 32 bits | Signed 64-bit values preserved |
| `negative.fn` | Negative print argument loses sign extension | `-42` |
| `short_circuit.fn` | Skipped right-hand expression still executes | RHS has no side effects |
| `signed_arithmetic.fn` | Boundary/signed arithmetic differs | Exact i64 fixture output |

These are reported differences, not ignored Rust failures. The C compiler is a
useful comparison, but specification-derived expected output is the correctness
oracle. Wrapping arithmetic fixture expectations pin the prototype's current QBE
behavior; a complete total-arithmetic error policy remains outside this experiment.
The default compiler's bugs still need fixes independent of migration.

## Measurement method

Measurements use the same generated 101-function program (a chain returning 100),
one warmup per check/emit action, 15 samples per frontend action, and three native
builds per compiler. Every resulting executable must print `100` and exit zero.
Timings include process startup; native builds include QBE, assembly, and linking.
The script records raw samples, SHA-256 hashes, host/tool versions, and caveats.
There is no CPU isolation, and fixed C-then-Rust order can affect caches. This is a
local feasibility experiment on a small shared subset.

Reproduce release measurements and native differential output:

```sh
mise run rust-evaluate
python3 scripts/test_rust_frontend.py \
  --rust-bin compiler-rs/target/release/fern-rs \
  --report build/rust-native-comparison.json
```

On the measurement host, a stale inherited `LIBRARY_PATH` contains a nonexistent
GCC directory. Native gates use `env -u LIBRARY_PATH`; the evaluation script removes
that variable itself. No compiler diagnostics are filtered from the native oracle.

## Local release results

macOS arm64 (Darwin 24.6.0), 12 logical CPUs, Apple clang 16.0.0, Rust/Cargo
1.75.0. C uses the existing release recipe (`-O2` frontend; QBE's existing flags).
Rust uses Cargo release defaults with symbol stripping. Both link the same
release C runtime. [Raw measurements and artifact hashes](../reports/rust-frontend-evaluation-2026-09-05.json)
identify this run, taken from the working tree before committing the experiment.

| Measurement | C | Rust |
| --- | ---: | ---: |
| Check median / p95 | 2.84 / 3.30 ms | 3.13 / 4.23 ms |
| Emit median / p95 | 4.27 / 5.00 ms | 3.42 / 5.27 ms |
| Native build median / largest sample | 111.91 / 114.31 ms | 98.98 / 639.60 ms |
| Compiler executable | 549,384 bytes (QBE included) | 535,656 bytes |
| Separate QBE helper | Included | 314,040 bytes |
| Compiler + backend total | 549,384 bytes | 849,696 bytes |
| Generated program | 397,696 bytes | 396,312 bytes |

Rust has slightly lower median emit and native build times here, but slower check
times and a large native-build outlier. Three native samples cannot establish a
latency distribution; the script's nearest-rank p95 is simply the maximum. These
results show feasible interactive latency, not a broad speed advantage. The Rust
bundle is about 55% larger before the shared runtime and system libraries. The
small native executable size difference does not establish a code-quality win.

## Compiler development cost

An isolated copy of the final Rust crate was built offline, first with an empty
target directory, then unchanged, then after appending one comment to `lib.rs`.
Each phase is one wall-clock sample, so differences below are exploratory.
[Build-cost records](../reports/rust-build-costs-2026-09-05.json) include the method.

| Rust crate build | Debug | Release |
| --- | ---: | ---: |
| Clean target | 1.45 s | 1.30 s |
| No source change | 0.021 s | 0.020 s |
| One source comment change | 0.473 s | 1.282 s |

Two runs of `mise run _build-fern release` took 3.97 s and 3.91 s. That recipe recompiles
the complete C frontend, support libraries, and QBE every time; there is no
incremental object-reuse path in the current recipe. It excludes the runtime.
The Rust measurements exclude QBE/runtime and cover a much smaller implementation,
so they establish an acceptable local edit/build loop, not a language-level build
speed advantage. Debug being slower than release in the clean samples also
illustrates why single samples should not support fine-grained comparisons.

To repeat Rust build phases without altering the working source:

```sh
fern_measure_dir="$(mktemp -d)"
cp compiler-rs/Cargo.toml compiler-rs/Cargo.lock "$fern_measure_dir/"
cp -R compiler-rs/src "$fern_measure_dir/src"
/usr/bin/time -p cargo build --locked --offline --manifest-path "$fern_measure_dir/Cargo.toml"
# Repeat unchanged, then append a comment in the copied src/lib.rs and repeat.
# Run the same three phases with --release for a separate empty release target.
```

The temporary directory can be removed after measurement. None of these build
costs includes installing toolchains or downloading native dependencies.

## Migration criteria

1. Extend typed IR to algebraic types, collections, pattern matching, generics,
   Result propagation, and modules, using executable specification fixtures at
   each step. Share the fixtures between frontends and fix discovered C bugs.
2. Preserve diagnostics and everyday tools: formatter, REPL, LSP, doc tests,
   package/build/install workflows, and all supported stdlib operations.
3. Expand fuzzing across parser/checker/emitter and native execution; validate
   ownership/GC boundaries for collection values and error paths.
4. Rerun performance and memory measurements on representative larger programs,
   both supported host platforms, clean/incremental builds, and complete bundles.
5. Switch the default only after full parity gates and a release migration plan
   pass. Keep QBE and the C runtime until separate evidence justifies changing them.

Rust is now a working migration candidate. This experiment provides no comparative
Odin result and no reason to start a third implementation before evaluating the
remaining Fern features in the typed pipeline.

See [decision 45](../../DECISIONS.md), the [roadmap](../../ROADMAP.md), and the
[documentation index](../README.md).
