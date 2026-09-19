# Sparse fragment adoption diagnostic

Status: candidate deferred. This is preserved experimental evidence, not an
adopted throughput improvement or a BEAM parity result. Base: `6afd962` plus
descriptor caching. The candidate adds sparse fragment metadata insertion.

The adversarial operation-count oracle falls from 32,774 comparisons to 38 when
two source keys span 32,768 target entries. Ownership, byte accounting and exact
finalizer tests pass. The combined candidate passed 2,399 Rust tests/292 suites,
317 native fixtures, all compatibility/fuzz checks, 203 ThreadSanitizer tests and
unchanged actor replay. A bounded-comparison win did not establish a reliable
end-to-end throughput gain, so the patch was removed from active implementation.

## Immutable inputs

| Artifact | SHA-256 |
| --- | --- |
| Cache-only baseline | `c5c175164aa1ee39f5272e3688fcffdd34b132449a968142c26ae5c208ad2f52` |
| Cache plus fragment candidate | `82b67b1084d92cffecdff0b304857e91e0b0349c6bfa9f6b2711b4d9a0244118` |
| Candidate runtime archive | `6a0e9d4ce5554718054b89ff0975319c1725c3264a9acd7d8352292e539b0d37` |
| Compiler | `24722ab1799b6d188507540cf1f395c13b673a69ff54ac3e37a1631c1ce1ae2e` |
| Diagnostic runner | `8a7bd9fbc0fa77575df3d529b10037a0bb2d5a9bdf1f30de23e71d82b0276ef5` |
| Runner source | `66ed0cec79508a5424337707df3467b4d9fdad1efa586097a32af21074b97d83` |
| 321-input source manifest | `290fea92f6f9ab94f7b74365e34803764c09a4327f394502e9cdf3c51cc8d948` |
| Deferred patch | `5f934a398ad1731eaa7f207e09a2a31f8605924cd66552cbc8bc2b8da8f83f00` |

Frozen candidate files: `/tmp/morrow-actors-cache-fragment-20260919` and
`/tmp/libmorrow-cache-fragment-20260919.a`. The [candidate patch](../actors-parity-fragment-adoption-20260919/candidate.patch)
is retained with the first diagnostic. `inputs.txt` supplies exact paths and host.

## Method and validation

Unchanged Rust diagnostic: 32 clients, 500/5,000 requests, one/two/four schedulers,
pinned/stealing, reductions 1, one warmup and five rotated measured rounds. Builds,
tests and other BEAM runs were paused. All 144 unique processes have exact
independent outputs, empty stderr and complete three-event traces. All 432 raw
files, CSV timing/throughput and operation counts were independently verified.
Medians retain all five rounds; no outliers were discarded.

| Requests/client | Schedulers | Mode | Baseline ops/s | Candidate ops/s | Ratio |
| ---: | ---: | --- | ---: | ---: | ---: |
| 500 | 1 | pinned | 312,775 | 313,676 | 1.003 |
| 500 | 1 | stealing | 320,822 | 325,050 | 1.013 |
| 500 | 2 | pinned | 310,109 | 315,625 | 1.018 |
| 500 | 2 | stealing | 294,137 | 194,730 | 0.662 |
| 500 | 4 | pinned | 291,678 | 297,060 | 1.018 |
| 500 | 4 | stealing | 258,485 | 279,442 | 1.081 |
| 5,000 | 1 | pinned | 300,952 | 295,918 | 0.983 |
| 5,000 | 1 | stealing | 301,468 | 306,851 | 1.018 |
| 5,000 | 2 | pinned | 297,222 | 305,794 | 1.029 |
| 5,000 | 2 | stealing | 189,791 | 184,223 | 0.971 |
| 5,000 | 4 | pinned | 294,856 | 290,462 | 0.985 |
| 5,000 | 4 | stealing | 262,282 | 259,572 | 0.990 |

The [first diagnostic](../actors-parity-fragment-adoption-20260919/STATUS.md)
shows short S2 stealing at 0.662×. The [repeat](../actors-parity-fragment-confirm-20260919/STATUS.md)
shows that cell at 1.010×, but short S2 pinned at 0.777× and long S2 stealing
at 0.807×. Four-scheduler stealing improves in both runs. Short S2 stealing
observations span multiple throughput ranges in both binaries; a single median
does not explain their cause. These mixed results justify deferring the candidate,
not removing unfavorable observations or asserting a universal slowdown.
