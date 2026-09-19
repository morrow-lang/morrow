# Adaptive transfer validation diagnostic

Date: 2026-09-19. Source base: `c93576d`, with the Decision163 adaptive
validation change. Both compared artifacts are immutable; exact binary and
runner hashes, paths and protocol are in `inputs.txt`. The new workload was
explicitly linked against `/tmp/libmorrow-adaptive-transfer-20260919.a`
(SHA-256 `9b7f69a6e12bb87a54a100dd6c65cb0a5a8c26b83f223b1c2e42b1e0c810ef97`)
using compiler `a9aa5f4d431da1bef273ad3af578772f30f86e43dda465aae558f16c983dffae`.
`source-manifest.sha256` freezes 315 compiler/runtime/build inputs.

The repository's Rust runner performed 144 interleaved processes: 24 warmups
and 120 measured runs, with one warmup and five measured observations per cell.
All passed the independent exact-output oracle, emitted no stderr, closed all
streams and stayed within the existing timeout/output bounds. No builds or
tests ran during measurement. `raw/` retains 432 files; `summary.md` reports
medians without removing outliers. Timing is ready through verified summary;
process wall time is recorded separately. These Morrow-only comparisons do not
measure BEAM parity.

The full repository gate passed 2,382 Rust tests across 292 result suites,
317 native fixtures, 20 examples, 63 dynamic compatibility programs, 295 atomic
rejections and 64+192+231 fuzz cases. ThreadSanitizer passed 191 tests in 11.82 s;
the two long churn tests passed in the ordinary 193-test runtime suite. Default
actor replay retained trace `d4e402a412f11e2f`, 48,739 callbacks and no cleanup
residue. Six standalone Rust runner tests passed, including bounded descendant
pipe handling, and its optimized build passed with warnings denied.

Against checkpoint1's unconditional index, two-scheduler stealing improves
1.580× for 32×500 and 1.992× for 32×5,000. Four-scheduler stealing improves
1.289× and 1.339×. Pinned controls stay near their prior throughput except for
ordinary short-run variation. This intervention and the preceding profile
support eager index construction as the major new regression.

See the [original-baseline comparison](../actors-parity-adaptive-baseline-20260919/STATUS.md)
for the remaining gap after correcting that regression.
