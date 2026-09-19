# Corrected monitor baseline diagnostic

Status: complete; no broad two-to-threefold feature regression reproduced.
Old is descriptor-cache checkpoint 3d0ef41e; new is cafc91d2, including isolated
actors/monitors, ordinary-message admission compatibility, and initialized
control/root corrections. This measures their combined effect on unchanged
request/reply workloads, not the individual cost of a monitor operation.

Eleven of twelve median cells are within about 5% of the earlier checkpoint.
Long four-scheduler stealing falls from 249,878 to 221,147 operations/s (0.885×).
That regression remains visible. Neither this diagnostic nor the preserved
cross-run variation establishes its cause. The latest full BEAM acceptance
remains checkpoint3: zero of nine strict parity cells.

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
| 500 | 1 | pinned | 48.459 | 50.144 | 330179 | 319082 | 0.966 |
| 500 | 1 | stealing | 48.549 | 48.854 | 329563 | 327506 | 0.994 |
| 500 | 2 | pinned | 51.532 | 50.015 | 310489 | 319905 | 1.030 |
| 500 | 2 | stealing | 81.824 | 84.921 | 195541 | 188411 | 0.964 |
| 500 | 4 | pinned | 53.500 | 52.349 | 299067 | 305639 | 1.022 |
| 500 | 4 | stealing | 51.935 | 49.509 | 308078 | 323173 | 1.049 |
| 5000 | 1 | pinned | 648.611 | 653.783 | 246681 | 244730 | 0.992 |
| 5000 | 1 | stealing | 630.026 | 615.790 | 253958 | 259829 | 1.023 |
| 5000 | 2 | pinned | 564.749 | 573.001 | 283312 | 279232 | 0.986 |
| 5000 | 2 | stealing | 914.882 | 932.082 | 174886 | 171659 | 0.982 |
| 5000 | 4 | pinned | 545.720 | 569.654 | 293190 | 280872 | 0.958 |
| 5000 | 4 | stealing | 640.313 | 723.500 | 249878 | 221147 | 0.885 |
