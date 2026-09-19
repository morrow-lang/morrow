# Independent paired-diagnostic validation — 2026-09-19

All three directories pass: 144 unique complete keys, 24 warmups, 120 measured runs, 432 expected raw files with no extras, exact round rotation, exactly ready→summary→streams-closed timestamps, positive ready-to-summary durations, process wall time covering stream closure, and raw/CSV duration and throughput agreement within printed precision. All 432 stdout streams match independently enumerated replies and checksums; all 432 stderr streams are empty. Checksums: 32×500=16,000 replies and74,411,992,000; 32×5000=160,000 replies and745,199,920,000. Each of the four provenance records per directory matches the current file byte count and SHA256. All 36 summary cells agree with recomputed medians.

Independent validation used a temporary standard-library read-only script; its machine-readable observations are retained in `independent-validation.json`. No source edits, builds, benchmark processes or timing runs were performed.

## First versus confirmation small-copy ratios

These are the runner’s specified **ratio of median throughputs**, not median paired-round ratios.

| Requests/client | Schedulers | Mode | First | Confirm | Candidate faster rounds: first / confirm |
| ---: | ---: | --- | ---: | ---: | ---: |
| 500 | 1 | pinned | 1.024 | 1.033 | 3/5 / 4/5 |
| 500 | 1 | stealing | 1.150 | 1.027 | 4/5 / 4/5 |
| 500 | 2 | pinned | 1.417 | 0.992 | 4/5 / 3/5 |
| 500 | 2 | stealing | 1.024 | 1.089 | 2/5 / 4/5 |
| 500 | 4 | pinned | 1.043 | 1.041 | 3/5 / 5/5 |
| 500 | 4 | stealing | 1.037 | 0.941 | 5/5 / 3/5 |
| 5000 | 1 | pinned | 1.094 | 1.065 | 3/5 / 5/5 |
| 5000 | 1 | stealing | 0.878 | 1.083 | 2/5 / 5/5 |
| 5000 | 2 | pinned | 1.390 | 1.088 | 4/5 / 5/5 |
| 5000 | 2 | stealing | 0.752 | 0.681 | 2/5 / 3/5 |
| 5000 | 4 | pinned | 1.073 | 1.079 | 4/5 / 2/5 |
| 5000 | 4 | stealing | 1.066 | 1.070 | 5/5 / 5/5 |

The first run has10/12 aggregate improvements; confirmation has9/12. Large first-run S2 pinned gains shrink (short1.417→0.992; long1.390→1.088), and long S1 stealing changes sign (0.878→1.083). Long S2 stealing remains materially lower by the specified aggregate (0.752→0.681), but the confirmation median paired-round ratio is1.063 with3/5 faster candidate rounds. Do not call that cell a uniformly reproduced per-round slowdown or prove a fragment/adoption cause. The aggregate acceptance concern remains valid; neither consistently broad gains nor a specific regression mechanism are established.

Long S2 stealing raw comparison:

| Run | Round | Old ms | New ms | New/old throughput |
| --- | ---: | ---: | ---: | ---: |
| first | 1 | 2100.491 | 1869.890 | 1.123 |
| first | 2 | 1156.790 | 1415.898 | 0.817 |
| first | 3 | 846.356 | 963.527 | 0.878 |
| first | 4 | 1093.741 | 1537.314 | 0.711 |
| first | 5 | 2092.417 | 1577.008 | 1.327 |
| confirm | 1 | 545.270 | 800.479 | 0.681 |
| confirm | 2 | 453.278 | 800.744 | 0.566 |
| confirm | 3 | 851.893 | 801.759 | 1.063 |
| confirm | 4 | 850.163 | 436.091 | 1.950 |
| confirm | 5 | 467.839 | 416.567 | 1.123 |

## Corrected monitor baseline comparison

Old descriptor-cache versus corrected monitor/root baseline:11/12 aggregate cells are within5% of1.0; long S4 stealing is0.885. This does **not** support the proposed broad2–3× monitor-feature regression. The exact same corrected-baseline binary/hash produces137,104 operations/s (first small-copy short S1 pinned),319,082 (monitor-baseline comparison), and330,773 (confirmation small-copy): a2.413× cross-run shift. The evidence establishes large absolute run-to-run variability, not its cause. Do not compare unmatched absolute medians to assign a feature cost.

Recommendation: preserve all three runs and defer the combined small-copy candidate. A separately verified memo-only experiment can isolate the changes, but these measurements alone do not identify fragment storage, adoption, allocator behavior, scheduler placement, or host state as the causal mechanism.
