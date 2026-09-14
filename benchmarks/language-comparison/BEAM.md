# Fern and Elixir on BEAM

This is a dated comparison. Subsequent [native arithmetic improvements](ARITHMETIC.md)
have their own paired measurements; the Elixir timings below have not been rerun.

Measured 2026-09-14 on Apple M4, 24 GiB RAM, macOS 26.5.1 ARM64. Fern compiler
and runtime are from `48ca830a687fd8e95077b4385317ae6ddabdc7b5`, after the
[native optimization work](NATIVE_OPTIMIZATION.md). Elixir **1.20.4** runs on
Erlang/OTP **29.0.6**, ERTS **17.0.6**, with the ARM64 **JIT enabled** and default
ten online schedulers. Rust uses the repository's pinned nightly, optimized with
`-O -C strip=symbols -C panic=abort`.

Fern is faster and uses less process memory on these particular scalar and
immutable-list workloads. This does not establish a faster actor system or a
replacement for OTP. Elixir executes on BEAM; these are Elixir/BEAM measurements,
not a separate Erlang-source benchmark.

## Repeated application work

Each process performs the same workload ten times. Values below are median
whole-process elapsed time divided by ten, including startup, output and shutdown.
Nothing is estimated or subtracted. These rows amortize startup; the following
in-VM measurements remove it entirely for Elixir.

| Workload, per repetition | Fern | Rust | Elixir tuples | Elixir structs |
| --- | ---: | ---: | ---: | ---: |
| 10,000 immutable updates, 256 cells | 6.93 ms | 1.21 ms | 31.07 ms | 32.09 ms |
| 100,000 immutable updates, 256 cells | 63.29 ms | 9.42 ms | 195.71 ms | 215.28 ms |
| 20 million scalar recurrence steps | 76.86 ms | 55.72 ms | 113.03 ms | 111.99 ms |

The two Elixir scalar columns run identical code in separate rotated samples;
they are a useful indication of ordinary measurement variability, not different
scalar representations. Rust remains faster, especially for contiguous immutable
model updates.

### BEAM without VM startup

Elixir's monotonic clock separately times each complete workload inside an already
running VM. Construction, updates, checksums and ordinary garbage collection are
included; output, oracle checking and VM startup/shutdown are excluded. No forced
collection or heap tuning is applied.

| Workload | Elixir tuples | Elixir structs |
| --- | ---: | ---: |
| 10,000 immutable updates | 18.29 ms | 20.24 ms |
| 100,000 immutable updates | 184.27 ms | 207.23 ms |
| 20 million scalar steps | 101.42 ms | Same scalar implementation |

Even comparing these Elixir compute timings against Fern's more inclusive batch
timings, Fern takes about **2.9–3.3× less time** for the larger model and **1.3×
less time** for scalar arithmetic. That is a conservative comparison of intervals,
not a claim that native CPU-only timings were measured. For short jobs, the
remaining Elixir startup contribution in ten-repeat batches is still substantial.

## Fresh process / CLI use

These are fresh-process invocations of the precompiled programs, with one
workload each. Compilation is outside every timed interval. The Elixir launcher
evaluates only the entry expression `FernComparison.main()`; it does not compile
the workload source on each launch.

| Workload | Fern | Rust | Elixir tuples | Elixir structs |
| --- | ---: | ---: | ---: | ---: |
| Zero-step scalar, print seed, exit | 3.03 ms | 2.58 ms | 120.16 ms | 117.85 ms |
| 10,000 immutable updates | 9.14 ms | 3.58 ms | 142.12 ms | 139.68 ms |
| 100,000 immutable updates | 60.72 ms | 11.72 ms | 302.08 ms | 322.47 ms |
| 20 million scalar steps | 77.60 ms | 57.52 ms | 223.49 ms | 217.16 ms |

The first row measures this Elixir application invocation, not minimal `erl`
startup or the startup of a tuned OTP release. It matters for short-lived CLI
tools but should not be used to rank running web servers.

For ten repetitions of the 100,000-update model, median peak process RSS is
**4.13 MiB Fern**, **1.56 MiB Rust**, and **82.70 / 82.73 MiB Elixir**
(tuples/structs). A single model run uses about **3.55 MiB Fern** and
**83 MiB Elixir**. These are whole-process high-water marks, not per-actor heap
sizes or memory growth extrapolations.

The Fern batch executable is **567,720 bytes**, versus **359,776 bytes** for Rust.
Elixir compiles to small BEAM modules that require the VM and libraries. The
privately extracted Erlang/Elixir bottles occupy **263 MiB**, but that includes
development and optional components: it is **not** a measured minimal deployable
release size. Neither artifact size nor RSS should be compared as if the VM's
schedulers and services supplied no additional functionality.

## Workload contract and verification

- Scalar: runtime-input recurrence `value * 48271 % 2147483647`, 20 million steps,
  seed 7. The independent modular-exponentiation oracle gives **468592691**.
  Intermediates fit each runtime's ordinary integer range; this is not an
  arbitrary-precision stress test. Precision checks separately preserve values
  above JavaScript's exact Number range.
- Model: rebuild a 256-cell collection using map for every update, updating
  `(seed + step * 17) % 256`. Keep and inspect the original zero-valued collection
  after computing the new model's weighted checksum. No implementation substitutes
  an in-place update, indexed update or different algorithm. The oracle counts
  visits using the modular inverse of 17 rather than repeating the measured loop.
- Fern uses records in its managed array-backed List. Rust maps a borrowed
  `Vec<Cell>` into a new vector with inline 16-byte cells. Elixir uses a linked
  list of `{index, value}` tuples, plus a separately reported idiomatic named-struct
  variant. Unchanged Elixir/Fern cells can be shared. Equal observable work does
  not mean equal physical layouts. The initial-alias oracle does not establish
  preservation of every intermediate historical version.
- Rust obscures inputs and results with `black_box` for each repetition to prevent
  hoisting identical scalar work out of the loop. Fern and Elixir execute their
  runtime-input functions on every repetition. Historical benchmark sources and
  reports are unchanged; new batch programs supply the repeat entry point.

The Rust harness checks **1,744 results**, including every timed output, 196 BEAM
boundary cases and repeated native scalar/model/precision cases. Seven measured
rounds follow one retained warmup; implementation order rotates each round.
There are **224 whole-process timing samples** and **40 supplementary in-VM
samples**, including warmups. Warmups and raw stdout/stderr are retained, never
silently trimmed. No project build/test ran during timing. Normal desktop apps
remained open; there is no CPU isolation, statistical confidence interval,
cross-machine claim or performance threshold in CI.

A separate Rust audit checked every recorded timed/boundary output and summary
median, using full 256-step cycles plus a residual target sum for the model
oracle. CI compiles/tests the Rust harness on Linux and macOS, and compiles and
executes the 196-case Elixir oracle grid on Ubuntu with these pinned versions.

The local `cargo xtask check` passed formatting, notices, Clippy, **2,193 Rust
tests**, **311 native fixtures**, **20 examples**, **63 dynamic compatibility
programs**, **295 atomic rejections**, and **64+192+231 fuzz cases**. The three
standalone Rust harness/oracle tests and Elixir warnings-as-errors compilation
also passed. No compiler or runtime implementation changed in this comparison.

## What this means for Fern's direction

The result supports native compilation, automatic memory and immutable values as
a useful combination: application code can stay functional without necessarily
paying more than BEAM for this kind of computation. Fern's quick startup and small
native artifact are particularly useful for the CLI side of the project vision.

BEAM/OTP remains the stronger reference for production concurrency. Its runtime
provides lightweight processes and multiple scheduler threads; messages between
nodes use Erlang's external term format. Local messages generally copy data,
with exceptions for reference-counted binaries and literals. Those are meaningful
design choices, not overhead we can simply remove from an equivalent system.
See the official [process efficiency guide](https://www.erlang.org/doc/system/eff_guide_processes.html)
and [garbage collector description](https://www.erlang.org/doc/apps/erts/garbagecollection.html).

Fern's current actor continuations and bounded cluster forwarding have separate
correctness evidence. This experiment does **not** measure actor throughput,
mailbox latency, fairness under CPU pressure, process spawning, supervision
recovery or distributed failures. Fern still lacks general remote language PIDs,
dynamic cluster membership and replicated ownership/failover; see
[cluster scope](../../docs/CLUSTER.md) and
[actor continuation limits](../../docs/ACTOR_CONTINUATIONS.md).
The next useful BEAM comparison is sustained request/reply and lifecycle churn
under contention, with tail latency and recovery assertions alongside throughput.
Arithmetic speed alone does not establish the project's Elixir/Phoenix vision.

## Reproduce

Use the versions above; Elixir's supported OTP combinations are documented
[officially](https://elixir-lang.org/docs/). The exact private install paths,
bottle provenance, hashes and optional-component limitations are recorded in
[toolchain.md](results/beam-macos-arm64-20260914/toolchain.md).
From the repository root on macOS, with the pinned Rust toolchain and Elixir/OTP
on PATH:

```sh
cargo xtask build --release
FERN_RUNTIME_LIB="$PWD/target/release/libfern_runtime_native.a" \
  target/release/fern build benchmarks/language-comparison/programs/batch.fn -o /tmp/fern-beam-batch
rustc --edition=2024 --crate-name comparison -O -C strip=symbols -C panic=abort \
  -Dwarnings benchmarks/language-comparison/programs/batch.rs -o /tmp/rust-beam-batch
mkdir /tmp/fern-beam-modules
elixirc --warnings-as-errors -o /tmp/fern-beam-modules benchmarks/language-comparison/programs/workloads.ex
rustc --edition=2024 -Dwarnings benchmarks/language-comparison/src/beam.rs -o /tmp/fern-beam-runner
rustc --edition=2024 --test -Dwarnings benchmarks/language-comparison/src/beam.rs -o /tmp/fern-beam-oracles
/tmp/fern-beam-oracles
/tmp/fern-beam-runner /tmp/fern-beam-batch /tmp/rust-beam-batch \
  "$(command -v elixir)" /tmp/fern-beam-modules /tmp/fern-beam-new-results
```

Choose unused output paths. Unset `ERL_FLAGS`, `ERL_AFLAGS` and
`ELIXIR_ERL_OPTIONS` to reproduce default scheduler settings, and remove ambient
`LIBRARY_PATH` if it overrides the system linker environment. Compilation and
installation should finish before timed runs.

[Raw evidence](results/beam-macos-arm64-20260914/) includes all timing samples,
unmodified streams, compiler output module hashes, program/harness source hashes,
runtime versions and oracle results. Preserve these observations as dated local
evidence rather than general language performance guarantees.
