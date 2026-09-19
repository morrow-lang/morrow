# Checkpoint 1 request/reply regression isolation

Status: complete bounded diagnostic. This is not a BEAM comparison or a parity
result. It interleaves two immutable Morrow artifacts in one quiet session to
separate a code-dependent request/reply change from cross-run variance.

## Inputs and protocol

| Artifact | Path at capture | SHA-256 |
| --- | --- | --- |
| Previous complete matrix | `/tmp/morrow-actors-release-20260919-transfer-gc-final` | `9de8abc625272d941a9e42aaa7ad7a25d09fe48e08012d8c78ed66327bcd0556` |
| Checkpoint 1 | `/tmp/morrow-actors-parity-checkpoint1-20260919` | `7668ab2fc66a13c51bde232fd3b56a13b161749291d896441c7f7dead77d693c` |
| Unchanged Rust runner | `/tmp/morrow-actor-runner-release` | `15d02e6885872865dd5aeccc179fff54aa20c511d21e6dd1c238dde875fff2e6` |

The Rust runner has no direct-cell mode, so a temporary Python standard-library
capture script at `/tmp/morrow-rr-regression-diagnostic-20260919-run.py`
provided bounded orchestration while preserving the runner's primary timing
definition: `ready` through the verified request/reply summary. It also retained
process wall time, raw streams and observed event timestamps. The script is not
repository tooling and is not part of this evidence directory or its manifest.
Its SHA-256, captured before the first process, is
`ae8c79bc8bc6412e3c1351df9237f0e9bbd7de46389a9fcbca724d2d79ec4546`.

The diagnostic covers pinned and stealing modes at one, two and four schedulers.
It runs the unchanged `request-reply 32 500` input first and the additive
`request-reply 32 5000` input second. Each cell has one warmup and five measured
rounds, with the four artifact/mode variants rotated each round. Reductions stay
at 1. Each child has a 30-second process-group bound, 1 MiB output bound and an
independent closed-form reply/checksum oracle.

All 144 process observations passed: 24 warmups and 120 measured runs. All stderr
was empty, no process timed out, and all 432 raw stream/event files are retained.
`inputs.json` records machine and artifact provenance; `files.sha256` covers the
complete diagnostic evidence except itself.

## Results

`summary.md` contains every median. The checkpoint/old throughput ratios are:

| Requests | Schedulers | Pinned | Stealing |
| ---: | ---: | ---: | ---: |
| 500 | 1 | 1.036 | 1.028 |
| 500 | 2 | 1.237 | 0.600 |
| 500 | 4 | 1.366 | 1.038 |
| 5,000 | 1 | 1.040 | 1.045 |
| 5,000 | 2 | 1.364 | 0.494 |
| 5,000 | 4 | 1.424 | 0.868 |

The two-scheduler stealing regression is code-dependent rather than cross-run
variance: it reproduces in the interleaved unchanged input and strengthens in
the ten-times-longer diagnostic. Checkpoint measured-round variability is 5.7%
for 500 requests and 2.0% for 5,000 requests, much smaller than the 40% and 51%
median losses.

The four-scheduler unchanged input does not reproduce the prior matrix's 17%
loss; checkpoint 1 is 3.8% faster than the old artifact in this interleaved run.
That earlier short-run difference is consistent with cross-run variance. The
longer input is 13.2% slower, so a smaller four-scheduler cost emerges when more
work is accumulated.

Pinned checkpoint 1 is 24--42% faster at two and four schedulers, while both
one-scheduler modes improve 3--5%. The regression is therefore isolated to
multi-scheduler stealing rather than the request/reply program, basic callback
execution or the new compiler artifact as a whole.

The result is consistent with the proposed hypothesis that eagerly building a
foreign-allocation interval index can cost more than direct live-word checks
after collection has reduced a transfer candidate to a small graph. It does not
prove that cause; the next step is a focused stack/profile or counters on the
checkpoint artifact before selecting an adaptive validation strategy.

Source provenance for the two artifacts remains in the preceding complete
matrices' `source-manifest.sha256` files. The main source tree was frozen and no
builds or tests ran during this diagnostic.
