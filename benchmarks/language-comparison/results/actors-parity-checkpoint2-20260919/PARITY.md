# Actor performance parity: checkpoint 2

Status: complete checkpoint; strict performance parity did not pass. Baseline
source commit: `b99ab2b0f737db60466dec65dd89262938eb457b`.

This checkpoint evaluates the next bounded optimization group against the
unchanged actor workloads and strict acceptance in the
[performance parity spec](../../../../docs/superpowers/specs/2026-09-19-actor-performance-parity.md).
Performance is one acceptance axis. The separate
[typed OTP process model](../../../../docs/superpowers/specs/2026-09-19-typed-otp-process-model.md)
defines broader core actor semantics; benchmark success cannot complete it.

## Candidate scope

- The runtime consumes a sparse GC `BTreeMap` only when a sweep sees at least
  64 blocks with at most 25% live.
- The compiler reuses a private frame only for a self-tail call returning
  `Unit` when owned captures are bit-identical, charging the old allocation cost
  before mutation.
- An exact sparse-sweep state oracle starts with 4,096 dead in-place removals
  and ends with zero dead in-place removals after the sweep.
- In the frame-reuse allocation oracle, 64 callbacks with 40-byte frames grow
  physical heap bytes by 2,560 before the change and leave physical heap bytes
  unchanged after it. The transient logical allocation charge remains intact
  while the physical frame garbage is removed.

The Morrow and Elixir workload sources, independent formulas, scheduler counts,
timeouts and measurement intervals remain frozen. This checkpoint does not add
new workloads.

## Release inputs

| Artifact | SHA-256 |
| --- | --- |
| Morrow workload source | `9f1a135ee9edad09686fec8bf23103098ddc34e83c4d63788f3170983074495b` |
| Elixir workload source | `648bebc86f56ed387a761100b223455e84b66599f7fd2c25eec9bd6b62bb1898` |
| Combined Morrow workload | `56e72093114fbe17655074093f1d91b46e208f251da9821264e57534d1d2e419` |
| Morrow compiler | `24722ab1799b6d188507540cf1f395c13b673a69ff54ac3e37a1631c1ce1ae2e` |
| Morrow runtime archive | `46fb536e1d7b38ac16271bed159fef84073f3310c194704fefe6662cc33846cd` |
| Rust runner | `15d02e6885872865dd5aeccc179fff54aa20c511d21e6dd1c238dde875fff2e6` |
| Frozen runner build source at `c93576d` | `37d2751ba819954d5cba2cdf06185aa30d2c46d316c60667695a93aea4311098` |
| Runner source in measurement checkout | `66ed0cec79508a5424337707df3467b4d9fdad1efa586097a32af21074b97d83` |
| Elixir BEAM module | `d75d5546be6f33144236458c46a5492eee717b364285e4ef653be3317e65511d` |
| 319-input source manifest | `334f436580e239844057865f74bda2366d3fbaa35639b9d581ffd4208821abc0` |

The immutable candidate files are
`/tmp/morrow-actors-reuse-sweep-20260919` and
`/tmp/libmorrow-reuse-sweep-20260919.a`. The smoke and matrix `inputs.txt` files
retain exact paths, sizes, protocol, host and tool versions. The runner's
original smoke and matrix manifests captured 318 inputs and omitted
`crates/morrow/tests/actor_frame_reuse/native.rs`; their retained manifests are
the supplied verified 319-input capture, which supplements that missing test
file and hashes to the value above.

The frozen matrix runner predates the additive Morrow-only diagnostic CLI mode.
`inputs.txt` hashes the source found in the measurement checkout; the separate
build-source hash above comes from the same earlier runner provenance recorded
in checkpoint1. The matrix executable and protocol remain unchanged.

## Verification and measurement integrity

Focused checks and the full gate passed: 2,393 Rust tests across 292 suites,
317 native fixtures, 20 examples, 63 dynamic compatibility programs, 295
rejection cases, and fuzz groups of 64, 192 and 231 cases. The runtime suite
passed 199 tests. ThreadSanitizer passed 197 tests in 11.77 seconds, with the two
long churn cases explicitly filtered. Deterministic actor replay produced trace
`d4e402a412f11e2f`, 48,739 callbacks and zero cleanup failures.

All agents paused builds and tests for measurement. No build, test or BEAM
process was present before the measurement window. The release semantic smoke
then passed all 36 exact-output processes, including the optional reductions=32
diagnostic variant.

An initial invocation failed while creating the exclusive evidence directory
because its parent directory did not exist. `setup-failure.log` preserves that
runner panic. It occurred before any child process started and is not an oracle
failure. After creating the parent, the smoke and matrix were run fresh.

The quiet `matrix/` contains 216 process observations: 36 warmups and 180
measured runs. All 216 unique run keys passed their exact output oracle, all
stderr was empty, every trace reached `streams-closed`, no child timed out, and
all 648 expected raw files are present. Every contention trace contains probes
0 through 256; the load-covered interval count derived from each trace matches
its CSV row. Default-stealing Morrow retained all 256 adjacent intervals in
every contention run. Medians use all five measured rounds per cell; no
observation or outlier was removed.

## Strict parity result

Strict parity requires default-stealing Morrow with reductions 1 to reach at
least `1.00x` the BEAM median in every one of the nine unchanged
workload/scheduler cells. Ratios from `0.80x` through `<1.00x` are competitive,
not parity. Checkpoint 2 passes zero of nine cells:

| Workload | Schedulers | Morrow operations/s | BEAM operations/s | Ratio | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| Request/reply | 1 | 299,849 | 1,144,216 | 0.262 | Below competitive |
| Request/reply | 2 | 186,020 | 2,640,754 | 0.070 | Below competitive |
| Request/reply | 4 | 274,749 | 1,567,942 | 0.175 | Below competitive |
| Contention scenario | 1 | 5,957 | 15,961 | 0.373 | Below competitive |
| Contention scenario | 2 | 4,509 | 23,615 | 0.191 | Below competitive |
| Contention scenario | 4 | 4,867 | 23,906 | 0.204 | Below competitive |
| Lifecycle scenario | 1 | 82,157 | 144,763 | 0.568 | Below competitive |
| Lifecycle scenario | 2 | 20,132 | 226,694 | 0.089 | Below competitive |
| Lifecycle scenario | 4 | 42,418 | 188,754 | 0.225 | Below competitive |

For orientation, the default-stealing medians below compare checkpoint 2 with
checkpoint 1. These are separate five-round quiet snapshots, not a causal
estimate of either optimization:

| Workload | Schedulers | Checkpoint 1 ops/s | Checkpoint 2 ops/s | Snapshot change |
| --- | ---: | ---: | ---: | ---: |
| Request/reply | 1 | 294,229 | 299,849 | +1.9% |
| Request/reply | 2 | 115,350 | 186,020 | +61.3% |
| Request/reply | 4 | 159,487 | 274,749 | +72.3% |
| Contention scenario | 1 | 1,224 | 5,957 | +386.8% |
| Contention scenario | 2 | 1,101 | 4,509 | +309.5% |
| Contention scenario | 4 | 1,185 | 4,867 | +310.6% |
| Lifecycle scenario | 1 | 82,420 | 82,157 | -0.3% |
| Lifecycle scenario | 2 | 26,670 | 20,132 | -24.5% |
| Lifecycle scenario | 4 | 29,014 | 42,418 | +46.2% |

The isolated request/reply diagnostics preserve mixed regressions, including
sparse-GC pinned S2 at `0.808` short and `0.900` long, and private-frame-reuse
long stealing S2 at `0.837`. They help direct profiling but do not assign the
checkpoint-matrix differences to one change.

Checkpoint 2 does not substantiate defined-matrix performance parity, broader
actor-runtime parity, or OTP feature parity. The nine-round confirmation gate
is not eligible because zero of nine strict median ratios passed.
