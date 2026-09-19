# Sparse-sweep request/reply diagnostic

Status: complete diagnostic. The adaptive sparse-sweep change was mixed and is
not a parity claim. Source base:
`b99ab2b0f737db60466dec65dd89262938eb457b`.

This isolated comparison holds the compiler and request/reply workload fixed
while comparing the adaptive-transfer binary with the GC-only sparse-sweep
binary. The collector consumes its sparse `BTreeMap` when a sweep sees at least
64 blocks with at most 25% live. The exact state oracle starts with 4,096 dead
in-place removals and ends with zero dead in-place removals after the sweep.

## Immutable inputs

| Artifact | SHA-256 |
| --- | --- |
| Adaptive-transfer workload | `f43d251bde234899458050c67d7582cdd031fcf2f6b29f594aad05eced066038` |
| Sparse-GC workload | `bd73c1b2b256d083d0cd865c7be4e044e84f38618767ad9f54897710b28f947b` |
| Sparse-GC runtime archive | `7e10b3f19c12f9ce4af90c6aaba328024312745ae08ccfa4e5199033b24692ba` |
| Diagnostic runner | `8a7bd9fbc0fa77575df3d529b10037a0bb2d5a9bdf1f30de23e71d82b0276ef5` |
| Runner source | `66ed0cec79508a5424337707df3467b4d9fdad1efa586097a32af21074b97d83` |
| 316-input source manifest | `6d531800d5f1005a7b7ab3c9b98c6f4b28dd75cf4ad378437086acba90079416` |

The immutable candidate files are
`/tmp/morrow-actors-sparse-gc-20260919` and
`/tmp/libmorrow-sparse-gc-20260919.a`. `inputs.txt` records the exact paths,
sizes, protocol and host. `source-manifest.sha256` is the captured 316-input
build manifest.

## Method and validation

The diagnostic runs request/reply with 32 clients, 500 and 5,000 requests per
client, one, two and four schedulers, pinned and default-stealing modes, and
reductions 1. Each cell has one warmup plus five measured rounds. Old/new and
mode order rotate by round. Ratios are candidate/baseline throughput ratios
computed from the two five-round medians; no observation or outlier was
removed.

All 144 unique process keys have the exact expected output, empty stderr and a
complete `ready`, `summary`, `streams-closed` trace. Every measurement row
matches its raw files, operation count, ready-to-summary interval and derived
throughput. The directory contains all 432 expected raw files.

| Requests/client | Schedulers | Mode | Baseline ops/s | Sparse-GC ops/s | Ratio |
| ---: | ---: | --- | ---: | ---: | ---: |
| 500 | 1 | pinned | 261,946 | 251,017 | 0.958 |
| 500 | 1 | stealing | 264,323 | 265,548 | 1.005 |
| 500 | 2 | pinned | 222,427 | 179,635 | 0.808 |
| 500 | 2 | stealing | 116,035 | 157,150 | 1.354 |
| 500 | 4 | pinned | 227,716 | 230,665 | 1.013 |
| 500 | 4 | stealing | 179,563 | 196,049 | 1.092 |
| 5,000 | 1 | pinned | 199,595 | 227,839 | 1.142 |
| 5,000 | 1 | stealing | 198,853 | 198,339 | 0.997 |
| 5,000 | 2 | pinned | 199,160 | 179,339 | 0.900 |
| 5,000 | 2 | stealing | 127,369 | 147,190 | 1.156 |
| 5,000 | 4 | pinned | 144,812 | 165,378 | 1.142 |
| 5,000 | 4 | stealing | 144,336 | 136,673 | 0.947 |

The sparse sweep helps several cells, but the pinned two-scheduler cells retain
the measured regressions (`0.808` short and `0.900` long), and long
four-scheduler stealing is `0.947`. This diagnostic does not establish a causal
effect for the combined checkpoint and does not replace the unchanged BEAM
parity matrix.
