# Small-copy allocation experiment

Status: deferred after the [repeat](../actors-parity-small-copy-confirm-20260919/STATUS.md).
The candidate combines two inline fragment entries and two inline copy-memo
entries, spilling larger graphs into the original collections. Independent
allocation tests eliminate temporary tiny-graph metadata allocations. The
candidate passes the full 2,445-test Rust gate across 295 suites, native,
examples, compatibility and fuzz checks; TSan passes 237 tests with two long
churn tests covered normally; deterministic actor replay remains exact.
These correctness and allocation results do not establish a throughput win.

Ten of twelve median cells improve in this first run. Long stealing regresses
to 0.878× at one scheduler and 0.752× at two. The repeat still regresses at two
schedulers (0.681×), so the combined production change is not retained.
`deferred-candidate.patch` preserves the exact source experiment against cafc91d2.
Its SHA-256 is `37b73a8bd92c8fa3c14c578405ea456c51a142b2fba7a58dd05a4dc319fbdbac`.

The old workload is the corrected monitor baseline cafc91d2, with full-width
message tags and precise Actor payload roots. The new workload adds only this
candidate. `baseline-source-inputs.sha256` and `candidate-source-inputs.sha256`
preserve the separately frozen source sets. The correctness corrections remain
accepted independently of this deferred optimization.

## Method and evidence

The unchanged diagnostic runs 32 clients with 500 and 5,000 requests each,
one/two/four schedulers, pinned and stealing, reductions 1, one warmup and five
rotated measured rounds. Builds, tests and other benchmark processes were
paused. All measured samples are retained; no outliers are removed. This is a
Morrow-to-Morrow diagnostic, not the frozen nine-cell BEAM acceptance matrix.

`inputs.txt` records the frozen workload and runner hashes. `summary.md` uses
all five measured medians in each cell. Raw stdout, stderr and event timestamps
are preserved for every process. Independent review verifies all 144 unique
keys, 24 warmups, 120 measured runs, 432 raw files, exact checksums, empty stderr,
event order, timing/throughput arithmetic and all twelve summary medians.

| Requests/client | Schedulers | Mode | Old median ready-to-summary ms | New median ready-to-summary ms | Old median operations/s | New median operations/s | New/old throughput |
| ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 500 | 1 | pinned | 116.700 | 113.925 | 137104 | 140443 | 1.024 |
| 500 | 1 | stealing | 124.092 | 107.890 | 128936 | 148299 | 1.150 |
| 500 | 2 | pinned | 194.034 | 136.901 | 82460 | 116873 | 1.417 |
| 500 | 2 | stealing | 230.881 | 225.540 | 69300 | 70941 | 1.024 |
| 500 | 4 | pinned | 140.111 | 134.383 | 114195 | 119063 | 1.043 |
| 500 | 4 | stealing | 174.596 | 168.383 | 91640 | 95022 | 1.037 |
| 5000 | 1 | pinned | 1792.581 | 1638.748 | 89257 | 97635 | 1.094 |
| 5000 | 1 | stealing | 1699.377 | 1936.143 | 94152 | 82639 | 0.878 |
| 5000 | 2 | pinned | 1327.494 | 955.310 | 120528 | 167485 | 1.390 |
| 5000 | 2 | stealing | 1156.790 | 1537.314 | 138314 | 104078 | 0.752 |
| 5000 | 4 | pinned | 833.991 | 777.340 | 191848 | 205830 | 1.073 |
| 5000 | 4 | stealing | 1004.500 | 942.599 | 159283 | 169743 | 1.066 |

The [independent review](../actors-parity-small-copy-20260919/INDEPENDENT_REVIEW.md)
also retains paired-round ratios. In the repeat, long two-scheduler stealing
is faster in three of five paired rounds, with median paired ratio 1.063,
despite its 0.681 ratio of median throughputs. The aggregate acceptance concern
is sufficient to defer this candidate; it does not prove a uniform per-round
slowdown or a specific fragment, adoption, allocator, or host-state cause.
