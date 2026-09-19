# Exact bounded tail batching diagnostic

Status: accepted; all six contention medians improve 1.439–1.460×.
Baseline is the accepted unboxed-send artifact at 38565dd6. The candidate changes
only compiler tail-batch work accounting. Each expansion removes a self-call
charged one, so whole-tree work is `1 + copies * (cost - 1)`. The unchanged
hot loop now executes three recurrence iterations per callback at 31 work units,
within the existing 32-unit budget; previously it executed two at 21 units.
The existing original-work <=16, eight-copy, node and depth bounds remain.

## Independent correctness and measurement

Red/green oracles assert expanded work 31, three actual native callback updates,
and a 3000-step source recurrence completing within 1100 polls while a sibling
finishes within 10, with precise GC and checksum 315511746. Existing full-width,
argument/fault/defer and ordinary-function ABI tests remain. The integrated full
gate passes 2451 Rust tests across 299 summaries,317 native fixtures, 20 examples,
63 compatibility programs, 295 atomic rejections and 64+192+231 fuzz cases.
All 36 release semantic smoke checks pass. The runtime archive is byte-identical
to accepted unboxed-send; its 232-test TSan run and exact 48739-callback actor
replay remain applicable. No runtime change warrants repeating those checks.

The additive temporary Rust diagnostic uses the frozen runner's unchanged
bounded subprocess handling and independent output oracles. `diagnostic-runner.rs`
preserves its source. Its seven tests pass, including the frozen 257-probe,
2000000-step matrix and exact checksum. At S1/S2/S4, it alternates old/new and
pinned/stealing order over one warmup and five measured rounds, reductions 1.
Builds/tests/other benchmark workloads were idle; no samples are removed.
Independent validation checks all 72 unique processes, 12 warmups, 60 measured,
216 raw files, exact output, empty stderr, trace timing/arithmetic and every
probe timestamp. Every S2/S4 stealing run preserves 256 load-covered intervals.
These are external observations, not internal mailbox latency.

| Frozen artifact | SHA-256 |
| --- | --- |
| Baseline workload | `f65a5b69db66ea4dcf8f8ac4bab74b7013ab86530e797673b32d51586a1a3ebf` |
| Candidate workload | `b0c6edaf00c0a7017061b095027bc740ff369becccba486286a38a7706e73d49` |
| Shared runtime archive | `48955ff56391f68cae2fa2baf3bc7e39023adeac2bc2164c2618e78931bdeeff` |
| Candidate compiler | `747d2ce5e89950b9bc2253b17706a1d2a93c5108037c47a92a1bf9e1328f6daa` |
| Candidate 331-input manifest | `9342314dbe4dabccf689c739615ae9011e0a5b7b33ba1ae4f610e21a1729055d` |
| Diagnostic runner | `56cce18129f1a09fcd1f6174506fa0350d2fed1bdde1ae52d9fc3d1ff7612091` |
| Diagnostic source | `7f8dc54f5720166166ae6e923c805b0a0884a16023f7d0221dc6bd05de9c8324` |

| Recurrence work | Schedulers | Mode | Old median ready-to-summary ms | New median ready-to-summary ms | Old median operations/s | New median operations/s | New/old throughput |
| ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 2000000 | 1 | pinned | 30.550 | 21.176 | 8412 | 12136 | 1.443 |
| 2000000 | 1 | stealing | 30.142 | 20.944 | 8526 | 12271 | 1.439 |
| 2000000 | 2 | pinned | 39.366 | 27.343 | 6529 | 9399 | 1.440 |
| 2000000 | 2 | stealing | 43.261 | 29.627 | 5941 | 8675 | 1.460 |
| 2000000 | 4 | pinned | 39.000 | 27.106 | 6590 | 9481 | 1.439 |
| 2000000 | 4 | stealing | 40.037 | 27.613 | 6419 | 9307 | 1.450 |

The subsequent [unchanged BEAM matrix](../actors-parity-checkpoint4-20260919/PARITY.md)
still passes zero of nine strict parity cells. This isolated contention gain
cannot substitute for that requirement.
