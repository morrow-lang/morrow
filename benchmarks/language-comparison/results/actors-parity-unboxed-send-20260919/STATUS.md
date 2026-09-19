# Immediately matched send-result optimization

Status: accepted request/reply diagnostic; all twelve medians improve.
This is not a BEAM parity result. Baseline is cafc91d2; the candidate changes
only immediate, nonescaping `match send(...)` lowering and its shared runtime
send entry. The combined small-copy experiment remains deferred.

## Behavior and verification

Eligible matches consume a private scalar outcome (zero success, current errors
3/4) instead of allocating a boxed Result. Validation, copied payloads, admission,
argument/guard order, Result duties, faults and roots remain shared with ordinary
send. Stored/returned Results, whole-Result bindings and unsupported patterns
retain the public boxed representation and RC ownership. No static Result or
public source operation is added.

Independent red/green allocation oracles count128→0 runtime Result blocks and
1→0 blocks in compiled source. Native tests cover actual actor delivery, full-width
Int/Float/String/Pid payloads, false then true guards, aliases, faults and cleanup
at S1/S2/S4 with stealing off/on and precise collection.

The integrated `cargo xtask check` passes2447 Rust tests across298 summaries,
317 native fixtures,20 examples,63 compatibility programs,295 atomic rejections
and64+192+231 fuzz cases. ThreadSanitizer passes232 tests, with the two long
churn tests covered normally. Actor replay remains `d4e402a412f11e2f`,48739
callbacks and zero cleanup residue. All36 release semantic smoke checks pass.

## Frozen artifacts

| Input | SHA-256 |
| --- | --- |
| Baseline workload | `28acad7ca55af8ea02eb8d6f3dac4927d71120aa2b03724c385e8fe3ea7dc7ef` |
| Candidate workload | `f65a5b69db66ea4dcf8f8ac4bab74b7013ab86530e797673b32d51586a1a3ebf` |
| Candidate runtime archive | `48955ff56391f68cae2fa2baf3bc7e39023adeac2bc2164c2618e78931bdeeff` |
| Candidate compiler | `2780e790bb57aeecf07e7a3420351183a27e5914f6c3a6ef9044dda15d6a7bc0` |
| Candidate331-input manifest | `51f4d8dcd93bd440e4f758aaa7807aa419731260c0e907cf55b51ea5c26b8f04` |
| Diagnostic runner | `8a7bd9fbc0fa77575df3d529b10037a0bb2d5a9bdf1f30de23e71d82b0276ef5` |

The paired diagnostic retains32 clients ×500/5000 requests, S1/S2/S4,
pinned/stealing, reductions1, one warmup and five rotated measured rounds.
No outliers are removed. Builds, tests and other benchmark workloads were idle.
Independent validation checks all144 unique processes,24 warmups,120 measured,
432 raw files, exact checksums, empty stderr, ordered event timestamps, CSV
arithmetic and all summary medians. `independent-validation.json` also preserves
paired-round ratios; the table uses the specified ratio of median throughputs.

| Requests/client | Schedulers | Mode | Old median ready-to-summary ms | New median ready-to-summary ms | Old median operations/s | New median operations/s | New/old throughput |
| ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 500 | 1 | pinned | 47.560 | 45.123 | 336417 | 354588 | 1.054 |
| 500 | 1 | stealing | 48.311 | 43.998 | 331184 | 363650 | 1.098 |
| 500 | 2 | pinned | 48.736 | 43.564 | 328302 | 367279 | 1.119 |
| 500 | 2 | stealing | 83.775 | 75.410 | 190987 | 212174 | 1.111 |
| 500 | 4 | pinned | 52.616 | 48.515 | 304091 | 329795 | 1.085 |
| 500 | 4 | stealing | 65.672 | 50.819 | 243635 | 314841 | 1.292 |
| 5000 | 1 | pinned | 547.448 | 508.310 | 292265 | 314769 | 1.077 |
| 5000 | 1 | stealing | 596.051 | 496.621 | 268433 | 322177 | 1.200 |
| 5000 | 2 | pinned | 512.459 | 491.894 | 312220 | 325273 | 1.042 |
| 5000 | 2 | stealing | 853.071 | 757.600 | 187558 | 211193 | 1.126 |
| 5000 | 4 | pinned | 537.157 | 507.673 | 297864 | 315164 | 1.058 |
| 5000 | 4 | stealing | 666.227 | 606.463 | 240159 | 263825 | 1.099 |

The range is1.042–1.292× over the corrected baseline. The latest completed
BEAM matrix remains checkpoint3 at zero of nine strict cells. These diagnostic
improvements do not substitute for a fresh unchanged matrix or its confirmation.
