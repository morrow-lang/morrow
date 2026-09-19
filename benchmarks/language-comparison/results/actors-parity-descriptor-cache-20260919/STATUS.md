# Descriptor-cache request/reply diagnostic

Status: complete diagnostic; all twelve measured cells improve in this run.
This is not a BEAM parity result. Source base: `6afd962` plus the descriptor-cache
change, without sparse fragment adoption.

The scheduler keeps eight exact-identity hints for immutable registered actor
function descriptors. Cache hits retain the original registry-position work
charge and exhaustion behavior. The independent callback oracle eliminates
4,096 table comparisons across 32 warm callbacks while preserving useful work;
collision, budget, independent-session and migration tests remain required.

## Immutable inputs

| Artifact | SHA-256 |
| --- | --- |
| Checkpoint2 workload | `56e72093114fbe17655074093f1d91b46e208f251da9821264e57534d1d2e419` |
| Cache-only workload | `c5c175164aa1ee39f5272e3688fcffdd34b132449a968142c26ae5c208ad2f52` |
| Cache-only runtime archive | `f1629ca6ecc2275aa8a837ad85a3bea3bd220595e512de02579534dd9c87cc48` |
| Compiler | `24722ab1799b6d188507540cf1f395c13b673a69ff54ac3e37a1631c1ce1ae2e` |
| Diagnostic runner | `8a7bd9fbc0fa77575df3d529b10037a0bb2d5a9bdf1f30de23e71d82b0276ef5` |
| Runner source | `66ed0cec79508a5424337707df3467b4d9fdad1efa586097a32af21074b97d83` |
| 320-input source manifest | `6e7491392dd204535952d6fa0a60278c2b24511bbb3e195d37461cecc7e5b03e` |

Frozen candidate files: `/tmp/morrow-actors-descriptor-cache-20260919` and
`/tmp/libmorrow-descriptor-cache-20260919.a`. The source manifest was captured
before applying fragment adoption. `inputs.txt` records paths, sizes and host.

## Method and validation

The unchanged Rust diagnostic runs 32 clients with 500 and 5,000 requests each,
one/two/four schedulers, pinned and default stealing, reductions1, one warmup
and five rotated measured rounds. Builds, tests and other BEAM processes were
paused throughout. Ratios use all five measured medians; no outliers are removed.

All 144 unique process keys pass independent exact output, empty stderr and
ready/summary/streams-closed trace checks. All 432 raw files are retained. Every
CSV operation count, ready-to-summary interval and throughput matches its trace.

| Requests/client | Schedulers | Mode | Checkpoint2 ops/s | Cache ops/s | Ratio |
| ---: | ---: | --- | ---: | ---: | ---: |
| 500 | 1 | pinned | 303,910 | 331,318 | 1.090 |
| 500 | 1 | stealing | 304,267 | 335,300 | 1.102 |
| 500 | 2 | pinned | 305,231 | 320,244 | 1.049 |
| 500 | 2 | stealing | 186,411 | 196,046 | 1.052 |
| 500 | 4 | pinned | 282,608 | 301,155 | 1.066 |
| 500 | 4 | stealing | 237,213 | 242,493 | 1.022 |
| 5,000 | 1 | pinned | 260,976 | 302,398 | 1.159 |
| 5,000 | 1 | stealing | 268,993 | 307,302 | 1.142 |
| 5,000 | 2 | pinned | 297,602 | 320,395 | 1.077 |
| 5,000 | 2 | stealing | 182,415 | 209,248 | 1.147 |
| 5,000 | 4 | pinned | 260,693 | 274,468 | 1.053 |
| 5,000 | 4 | stealing | 200,818 | 245,161 | 1.221 |

The improvement ranges from 1.022× to 1.221×. This isolated comparison does not
replace the unchanged BEAM matrix or establish broader OTP semantics.
