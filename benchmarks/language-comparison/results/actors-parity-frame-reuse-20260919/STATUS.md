# Private-frame reuse request/reply diagnostic

Status: complete diagnostic. Private compiler-frame reuse was mixed and is not
a parity claim. Source base:
`b99ab2b0f737db60466dec65dd89262938eb457b`.

This isolated comparison holds the request/reply workload fixed while comparing
the sparse-GC binary with the combined sparse-GC and private-frame-reuse binary.
A private compiler frame is reusable only for a self-tail call returning `Unit`
when its owned captures are bit-identical. The implementation charges the old
allocation cost before mutating the frame. In the exact allocation oracle, 64
callbacks with 40-byte frames grow physical heap bytes by 2,560 before the
change and leave physical heap bytes unchanged after it. The transient logical
allocation charge remains intact while the physical frame garbage is removed.

## Immutable inputs

| Artifact | SHA-256 |
| --- | --- |
| Sparse-GC workload | `bd73c1b2b256d083d0cd865c7be4e044e84f38618767ad9f54897710b28f947b` |
| Combined workload | `56e72093114fbe17655074093f1d91b46e208f251da9821264e57534d1d2e419` |
| Combined runtime archive | `46fb536e1d7b38ac16271bed159fef84073f3310c194704fefe6662cc33846cd` |
| Morrow compiler | `24722ab1799b6d188507540cf1f395c13b673a69ff54ac3e37a1631c1ce1ae2e` |
| Diagnostic runner | `8a7bd9fbc0fa77575df3d529b10037a0bb2d5a9bdf1f30de23e71d82b0276ef5` |
| Runner source | `66ed0cec79508a5424337707df3467b4d9fdad1efa586097a32af21074b97d83` |
| 319-input source manifest | `334f436580e239844057865f74bda2366d3fbaa35639b9d581ffd4208821abc0` |

The immutable candidate files are
`/tmp/morrow-actors-reuse-sweep-20260919` and
`/tmp/libmorrow-reuse-sweep-20260919.a`. `inputs.txt` records the exact paths,
sizes, protocol and host. `source-manifest.sha256` is the captured 319-input
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

| Requests/client | Schedulers | Mode | Sparse-GC ops/s | Combined ops/s | Ratio |
| ---: | ---: | --- | ---: | ---: | ---: |
| 500 | 1 | pinned | 302,895 | 289,867 | 0.957 |
| 500 | 1 | stealing | 297,983 | 296,462 | 0.995 |
| 500 | 2 | pinned | 301,229 | 302,852 | 1.005 |
| 500 | 2 | stealing | 184,465 | 187,424 | 1.016 |
| 500 | 4 | pinned | 284,899 | 282,936 | 0.993 |
| 500 | 4 | stealing | 263,464 | 279,317 | 1.060 |
| 5,000 | 1 | pinned | 280,544 | 276,543 | 0.986 |
| 5,000 | 1 | stealing | 280,028 | 282,198 | 1.008 |
| 5,000 | 2 | pinned | 291,716 | 275,332 | 0.944 |
| 5,000 | 2 | stealing | 216,462 | 181,087 | 0.837 |
| 5,000 | 4 | pinned | 283,826 | 281,913 | 0.993 |
| 5,000 | 4 | stealing | 219,094 | 222,451 | 1.015 |

The short stealing cells improve at two and four schedulers, but the long
two-scheduler stealing result retains the measured `0.837` regression. The
other ratios remain close to neutral. This diagnostic does not establish a
causal effect for the combined checkpoint and does not replace the unchanged
BEAM parity matrix.
