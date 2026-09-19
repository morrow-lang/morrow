# Small-copy experiment repeat

Status: complete; confirms the decision to defer the combined candidate.
This compares the same immutable artifacts as the
[first experiment](../actors-parity-small-copy-20260919/STATUS.md).
Nine of twelve cells improve; the long two-scheduler stealing cell falls from
293,433 to 199,880 operations/s (0.681×), following 0.752× in the first run.
Short four-scheduler stealing is 0.941×; short two-scheduler pinned is 0.992×.
No universal throughput improvement or parity is established.

Absolute throughput differs substantially across runs. For example, the same
baseline short one-scheduler pinned workload measures 137,104 operations/s in
the first run and 330,773 here. No environmental cause has been established.
Keep all runs and use their within-run comparisons; do not silently select the
faster run or infer a stage1-wide regression from the earlier absolute rates.

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
| 500 | 1 | pinned | 48.372 | 46.811 | 330773 | 341800 | 1.033 |
| 500 | 1 | stealing | 47.954 | 46.703 | 333652 | 342591 | 1.027 |
| 500 | 2 | pinned | 51.641 | 52.042 | 309829 | 307443 | 0.992 |
| 500 | 2 | stealing | 85.329 | 78.351 | 187510 | 204210 | 1.089 |
| 500 | 4 | pinned | 52.295 | 50.254 | 305957 | 318382 | 1.041 |
| 500 | 4 | stealing | 49.535 | 52.652 | 323002 | 303882 | 0.941 |
| 5000 | 1 | pinned | 528.906 | 496.675 | 302511 | 322142 | 1.065 |
| 5000 | 1 | stealing | 535.250 | 494.020 | 298926 | 323874 | 1.083 |
| 5000 | 2 | pinned | 522.747 | 480.305 | 306075 | 333122 | 1.088 |
| 5000 | 2 | stealing | 545.270 | 800.479 | 293433 | 199880 | 0.681 |
| 5000 | 4 | pinned | 578.246 | 536.022 | 276699 | 298495 | 1.079 |
| 5000 | 4 | stealing | 693.738 | 648.579 | 230635 | 246693 | 1.070 |

The [independent review](../actors-parity-small-copy-20260919/INDEPENDENT_REVIEW.md)
also retains paired-round ratios. In the repeat, long two-scheduler stealing
is faster in three of five paired rounds, with median paired ratio 1.063,
despite its 0.681 ratio of median throughputs. The aggregate acceptance concern
is sufficient to defer this candidate; it does not prove a uniform per-round
slowdown or a specific fragment, adoption, allocator, or host-state cause.
