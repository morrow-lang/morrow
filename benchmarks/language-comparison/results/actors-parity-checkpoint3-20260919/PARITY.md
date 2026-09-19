# Actor performance parity: checkpoint 3

Status: measurements complete; zero of nine strict parity cells pass.
Candidate: `6afd962` plus immutable descriptor caching. The sparse fragment
candidate was deferred after inconsistent throughput results. Performance and
[local OTP semantics](../../../../docs/superpowers/specs/2026-09-19-typed-otp-process-model.md)
remain separate acceptance axes.

## Change and inputs

Eight scheduler-local descriptor hints eliminate repeated successful registry
scans while retaining the original work charge and budget-exhaustion point.
The [144-process diagnostic](../actors-parity-descriptor-cache-20260919/STATUS.md)
improves all twelve request/reply cells by 1.022–1.221×. The
[fragment experiment](../actors-parity-fragment-adoption-20260919/STATUS.md) and
[repeat](../actors-parity-fragment-confirm-20260919/STATUS.md) retain their
regressions and the unadopted patch.

| Artifact | SHA-256 |
| --- | --- |
| Cache-only workload | `c5c175164aa1ee39f5272e3688fcffdd34b132449a968142c26ae5c208ad2f52` |
| Runtime archive | `f1629ca6ecc2275aa8a837ad85a3bea3bd220595e512de02579534dd9c87cc48` |
| Compiler | `24722ab1799b6d188507540cf1f395c13b673a69ff54ac3e37a1631c1ce1ae2e` |
| Frozen matrix runner | `15d02e6885872865dd5aeccc179fff54aa20c511d21e6dd1c238dde875fff2e6` |
| Runner source in checkout | `66ed0cec79508a5424337707df3467b4d9fdad1efa586097a32af21074b97d83` |
| Morrow workload source | `9f1a135ee9edad09686fec8bf23103098ddc34e83c4d63788f3170983074495b` |
| Elixir workload source | `648bebc86f56ed387a761100b223455e84b66599f7fd2c25eec9bd6b62bb1898` |
| 320-input manifest | `6e7491392dd204535952d6fa0a60278c2b24511bbb3e195d37461cecc7e5b03e` |
| Runner build source at c93576d | `37d2751ba819954d5cba2cdf06185aa30d2c46d316c60667695a93aea4311098` |
| BEAM module | `d75d5546be6f33144236458c46a5492eee717b364285e4ef653be3317e65511d` |

The frozen files are `/tmp/morrow-actors-descriptor-cache-20260919` and
`/tmp/libmorrow-descriptor-cache-20260919.a`. Smoke and matrix `inputs.txt`
record paths, sizes, host and pinned Elixir 1.20.4 / OTP 29.0.6. The matrix runner
predates the additive diagnostic CLI, so its build-source and checkout-source
hashes differ. The retained manifest includes the independent native frame-reuse
test omitted by the runner's source-only traversal.

## Measurement integrity

Builds, tests and other BEAM work were paused. Semantic smoke passes all 36
independent process checks. The matrix retains 216 unique processes: 36 warmups
and 180 measured runs, with 648 raw files. All outputs, empty stderr, operation
counts, trace-derived intervals and throughput rows were independently checked.
Request/reply and contention measure ready-to-summary; lifecycle deliberately
measures ready-to-streams-closed, including final shutdown. Every contention
trace has probes 0 through 256 and its covered-interval count matches the CSV.
All default-stealing runs retain all 256 adjacent intervals. These gaps are
external completion observations, not internal mailbox latency.

All five measured rounds contribute to medians. No outliers or unfavorable
observations were removed. `metadata-failure/` retains the first setup failure:
source traversal raced removal of the deferred fragment test and stopped before
any workload child launched. That attempt also recorded the rejected combined
archive. The completed smoke/matrix used the restored cache-only archive and
stable source snapshot; their hashes are authoritative.

## Correctness acceptance

Final cache-only `cargo xtask check` passes 2,397 Rust tests across 292 result
suites, 317 native fixtures, 20 examples, 63 compatibility programs, 295 atomic
rejections and 64+192+231 fuzz cases. The ordinary runtime suite has 203 tests.
ThreadSanitizer passes 201 in 13.66 seconds, excluding the same two long churn
tests covered in the ordinary gate. Cache-only actor replay retains trace
`d4e402a412f11e2f`, 48,739 callbacks and zero cleanup residue.

## Strict result

Default stealing with reductions 1 must reach at least 1.00× BEAM median in each
unchanged cell, then pass confirmation. Ratios 0.80–<1.00× are competitive. Every
cell remains below competitive:

| Workload | Schedulers | Morrow ops/s | BEAM ops/s | Ratio |
| --- | ---: | ---: | ---: | ---: |
| request-reply | 1 | 331,911 | 1,079,082 | 0.308 |
| request-reply | 2 | 194,437 | 2,652,447 | 0.073 |
| request-reply | 4 | 307,181 | 1,537,969 | 0.200 |
| contention | 1 | 8,573 | 16,113 | 0.532 |
| contention | 2 | 6,451 | 23,626 | 0.273 |
| contention | 4 | 6,403 | 23,451 | 0.273 |
| lifecycle | 1 | 86,740 | 199,970 | 0.434 |
| lifecycle | 2 | 24,032 | 249,587 | 0.096 |
| lifecycle | 4 | 26,161 | 179,525 | 0.146 |

The confirmation gate remains ineligible. Contention improves over checkpoint2's
separate snapshot, but lifecycle remains uneven: S4 Morrow falls from 42,418 to
26,161 ops/s. Separate snapshots are not causal estimates. These measurements
establish neither general actor-performance parity nor OTP feature parity.
