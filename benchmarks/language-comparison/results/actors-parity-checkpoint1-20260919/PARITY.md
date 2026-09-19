# Actor performance parity: checkpoint 1

Status: complete checkpoint; strict performance parity did not pass. Baseline source commit:
`11e6ad5b8bb6058dd61e22d25ef94440b5c5a2b2`.

This checkpoint evaluates the first bounded optimization group against the
unchanged actor workloads and strict acceptance in the
[performance parity spec](../../../../docs/superpowers/specs/2026-09-19-actor-performance-parity.md).
Performance is one acceptance axis. The separate
[typed OTP process model](../../../../docs/superpowers/specs/2026-09-19-typed-otp-process-model.md)
defines broader core actor semantics; benchmark success cannot complete it.

## Candidate scope

- Transport idle and empty-drain fast paths retain a locked shutdown fence.
- Heap-transfer validation uses a bounded foreign-allocation interval index.
- Stack validation reuses bounded scratch storage on its common path.
- The compiler emits bounded tail batches for native continuation preemption.

The Morrow and Elixir workload sources, independent formulas, scheduler counts,
timeouts and measurement intervals are unchanged. This checkpoint does not add
the future sharded or payload-copy workloads.

## Release inputs

| Artifact | SHA-256 |
| --- | --- |
| Morrow workload | `7668ab2fc66a13c51bde232fd3b56a13b161749291d896441c7f7dead77d693c` |
| Morrow compiler | `a9aa5f4d431da1bef273ad3af578772f30f86e43dda465aae558f16c983dffae` |
| Morrow runtime archive | `ffd818e825f4b273cadfd9f2e52418e0a30a45a9c42e23d5ea3723e200a6e3a1` |
| Rust runner | `15d02e6885872865dd5aeccc179fff54aa20c511d21e6dd1c238dde875fff2e6` |
| Elixir BEAM module | `d75d5546be6f33144236458c46a5492eee717b364285e4ef653be3317e65511d` |

The workload was linked explicitly against
`target/release/libmorrow_runtime_native.a`. The smoke runner's `inputs.txt` and
`source-manifest.sha256` retain exact paths, sizes and source hashes.

## Verification and results

The release semantic smoke passed all 36 exact-output processes, including the
optional reductions=32 diagnostic variant. ThreadSanitizer passed 189 tests in
18.60 seconds before the smoke; two long churn cases were excluded from that
sanitizer invocation. The complete repository gate subsequently passed 2,380
Rust tests across 292 suites, 317 native fixtures, 20 examples, 63 compatibility
programs, 295 rejection cases and the full fuzz matrix. Release and deterministic
replay checks also passed.

The quiet `matrix/` contains 216 process observations: 36 warmups and 180
measured runs. All 216 unique run keys passed their output oracle, all stderr was
empty, every event trace reached `streams-closed`, and no child timed out. The
directory retains 648 raw files and a verified 315-entry source manifest.

Strict parity requires default stealing Morrow with reductions 1 to reach at
least `1.00x` the BEAM median in every one of the nine unchanged
workload/scheduler cells. Ratios from `0.80x` through `<1.00x` are competitive,
not parity. Checkpoint 1 reached neither band in any cell:

| Workload | Schedulers | Morrow operations/s | BEAM operations/s | Ratio | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| Request/reply | 1 | 294,229 | 1,129,239 | 0.261 | Below competitive |
| Request/reply | 2 | 115,350 | 2,612,263 | 0.044 | Below competitive |
| Request/reply | 4 | 159,487 | 1,532,017 | 0.104 | Below competitive |
| Contention scenario | 1 | 1,224 | 15,710 | 0.078 | Below competitive |
| Contention scenario | 2 | 1,101 | 23,386 | 0.047 | Below competitive |
| Contention scenario | 4 | 1,185 | 23,227 | 0.051 | Below competitive |
| Lifecycle scenario | 1 | 82,420 | 187,620 | 0.439 | Below competitive |
| Lifecycle scenario | 2 | 26,670 | 213,407 | 0.125 | Below competitive |
| Lifecycle scenario | 4 | 29,014 | 203,919 | 0.142 | Below competitive |

Relative to the preceding complete Morrow matrix, the checkpoint bundle raised
default-stealing contention throughput by 109%, 111% and 117% at one, two and
four schedulers. Request/reply changed by +3%, -39% and -17%, while lifecycle
changed by -1%, -5% and -1%. These are two separate five-round quiet snapshots,
so the changes identify where to profile next rather than assigning each effect
to one optimization.

All two- and four-scheduler contention runs retained the full 256 adjacent probe
intervals under load. External marker gaps remain diagnostic rather than true
mailbox latency. The whole contention interval also includes the integer
recurrence, as required by the unchanged workload.

Checkpoint 1 therefore improves contention but does not substantiate defined-
matrix performance parity, broader actor-runtime parity, or OTP feature parity.
The nine-round confirmation gate is not eligible until a future unchanged matrix
passes all nine strict median ratios.
