# Actor performance parity: checkpoint4

Status: complete; **zero of nine strict parity cells pass**. No cell reaches the
0.80–<1.00 competitive band. The nine-round confirmation is not eligible.
Candidate is38565dd6 plus exact bounded tail batching. It includes isolated
processes/monitors and the message/root compatibility corrections; broader
links and supervision are being integrated separately.

## Changes and evidence

[Immediately matched send outcomes](../actors-parity-unboxed-send-20260919/STATUS.md)
remove Result allocations and improve all twelve paired request/reply medians
1.042–1.292×. [Exact tail-batch accounting](../actors-parity-tail-batch-exact-20260919/STATUS.md)
fits three iterations into 31/32 work units and improves all six paired contention
medians 1.439–1.460×. Combined inline fragment/memo storage remains deferred,
with both unfavorable diagnostics preserved. No workload, default reductions,
scheduler count, timing interval, bound or acceptance threshold changes.

The integrated compiler/runtime gate passes 2451 Rust tests across 299 summaries,
317 native fixtures, 20 examples, 63 compatibility programs, 295 atomic rejections
and 64+192+231 fuzz cases. All 36 release smoke checks pass. The runtime archive
is identical to accepted unboxed-send:232 TSan tests pass (two long churn tests
covered normally), and deterministic replay remains `d4e402a412f11e2f`, 48739
callbacks with zero cleanup residue. The compiler change has independent actual
native callback/fairness/GC/fault/ABI oracles.

## Measurement integrity and frozen inputs

The unchanged matrix retains 216 unique processes: 36 warmups and 180 measured,
with 648 raw streams/timestamp files. Runtime order rotates over all five measured
rounds; no observations are removed. Builds/tests and other benchmark workloads
were idle. Independent validation checks exact stdout/checksums, empty stderr,
operation counts, event order, trace-derived work intervals and throughput,
and contention interval counts/percentiles. All default-stealing contention runs
retain 256 adjacent probe intervals under load. Those gaps are external completion
observations, not internal mailbox latency.

Request/reply and contention measure ready-to-summary. Lifecycle measures
ready-to-streams-closed, including final shutdown. Pinned and reductions 32 remain
diagnostic controls; only stealing with default reductions 1 determines parity.

| Artifact | SHA-256 |
| --- | --- |
| Candidate workload | `b0c6edaf00c0a7017061b095027bc740ff369becccba486286a38a7706e73d49` |
| Runtime archive | `48955ff56391f68cae2fa2baf3bc7e39023adeac2bc2164c2618e78931bdeeff` |
| Compiler | `747d2ce5e89950b9bc2253b17706a1d2a93c5108037c47a92a1bf9e1328f6daa` |
| Frozen matrix runner | `15d02e6885872865dd5aeccc179fff54aa20c511d21e6dd1c238dde875fff2e6` |
| Runner build source at c93576d | `37d2751ba819954d5cba2cdf06185aa30d2c46d316c60667695a93aea4311098` |
| Checkout runner source | `66ed0cec79508a5424337707df3467b4d9fdad1efa586097a32af21074b97d83` |
| Morrow workload source | `9f1a135ee9edad09686fec8bf23103098ddc34e83c4d63788f3170983074495b` |
| Elixir workload source | `648bebc86f56ed387a761100b223455e84b66599f7fd2c25eec9bd6b62bb1898` |
| Candidate 331-input manifest | `9342314dbe4dabccf689c739615ae9011e0a5b7b33ba1ae4f610e21a1729055d` |
| BEAM module | `d75d5546be6f33144236458c46a5492eee717b364285e4ef653be3317e65511d` |

`inputs.txt` retains exact paths, sizes, Elixir 1.20.4 / OTP 29.0.6 and host.
The matrix runner predates the additive diagnostic CLI; its build-source hash
therefore differs from the checkout source. Runtime/compiled-workload files are
frozen separately in `/tmp`, and both captured source manifests are retained.

## Strict result

| Workload | Schedulers | Morrow ops/s | BEAM ops/s | Ratio |
| --- | ---: | ---: | ---: | ---: |
| request-reply | 1 | 349,106 | 1,148,662 | 0.304 |
| request-reply | 2 | 210,754 | 2,217,013 | 0.095 |
| request-reply | 4 | 249,521 | 1,576,905 | 0.158 |
| contention | 1 | 12,165 | 15,499 | 0.785 |
| contention | 2 | 8,702 | 23,314 | 0.373 |
| contention | 4 | 9,250 | 23,242 | 0.398 |
| lifecycle | 1 | 85,080 | 191,162 | 0.445 |
| lifecycle | 2 | 28,535 | 240,156 | 0.119 |
| lifecycle | 4 | 29,006 | 229,336 | 0.126 |

The one-scheduler contention case reaches 0.785× BEAM, up from 0.532× at
checkpoint3. Two/four-scheduler contention reaches 0.373/0.398×, previously
0.273/0.273×. The other workloads remain far below the target.

Absolute default-stealing request/reply changes from 331911/194437/307181 to
349106/210754/249521 operations/s at S1/S2/S4. In particular, S4 is 18.8% below
checkpoint3 despite the favorable separate send comparison. Preserve this
regression and the known run-to-run variation; these different-checkpoint
measurements do not establish its cause. Lifecycle changes from 86740/24032/26161
to 85080/28535/29006; its ratios also reflect changed BEAM medians. Do not hide
these outcomes behind the contention improvement.

Further allocation/copy and scheduler work remains, followed by a fresh unchanged
matrix. Confirmation requires all nine strict medians >=1.00 first. Local OTP
semantics, distribution, native preemption and broader workloads remain separate
acceptance boundaries.
