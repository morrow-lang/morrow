# Focused transfer-GC experiment

This bounded experiment compares the complete pre-change Morrow workload
artifact with a candidate that performs precise collection at the callback
boundary before heap transfer. It was used to decide whether the optimization
warranted the full repository gate and a new quiet 216-process matrix. It is not
a replacement for that matrix.

The experiment covers stealing with `MORROW_REDUCTIONS=1` at two and four
schedulers for the same full-size request/reply, contention and lifecycle inputs
as the language-comparison harness. Each cell alternates the before and candidate
artifacts over turns 0--3. Turn 0 is warmup; the table uses the median wall time
from turns 1--3. Every child had a 10-second bound. All 48 process outputs passed
independent formula checks and all stderr was empty.

| Workload | Schedulers | Before median wall ms | Candidate median wall ms | Before / candidate |
| --- | ---: | ---: | ---: | ---: |
| Request/reply | 2 | 1,281.630 | 85.060 | 15.07x |
| Request/reply | 4 | 1,040.200 | 95.932 | 10.84x |
| Contention | 2 | 492.213 | 498.047 | 0.99x |
| Contention | 4 | 509.958 | 512.942 | 0.99x |
| Lifecycle | 2 | 28.488 | 29.251 | 0.97x |
| Lifecycle | 4 | 27.095 | 28.065 | 0.97x |

The request/reply result removes most of the multi-scheduler stealing cost in
this focused run. Contention changed by about 1%, while lifecycle was 3--4%
slower. Three measured rounds are enough to accept the candidate for full
validation, but not to publish it as the final language comparison.

`before.sample.txt` is a 1 ms macOS stack sample of the two-scheduler pre-change
request/reply case. Across the main and scheduler threads, 633 of roughly 702
active samples were under `detach_heap`; most of those reached `Domain::owns`.
This identifies repeated ownership scanning during transfer as the measured
bottleneck. The full stack evidence is retained rather than reducing it to that
interpretation.

`binaries.json` records both workload hashes. `measurements.json` contains all
48 ordered observations and oracle outcomes. `raw/` retains each process stdout
and stderr; the top-level `before.stdout` and `before.stderr` belong to the
profiled invocation.

`migration-before-to-accepted.patch` is a forward unified patch from the
pre-collection `migration.rs` (`54a70cc51fb1d186b7dbdfd8f90afb79bbfed02c10c464675e2e9b5363a06f24`)
to the accepted source
(`c62dc05e698af4dfd9a480604daadb222d4e1d809b17d3b6cde9ce889d7c92e0`).
Its SHA-256 is
`05437e159689d23d60215f0d01368b8b8169a338bfc18da2e0643ea2122eaaa9`.
Reverse-applying it to the accepted repository source reconstructs the exact
pre-collection file whose hash appears in the checkpoint source manifest. The
temporary full source backup was therefore deliberately excluded.
