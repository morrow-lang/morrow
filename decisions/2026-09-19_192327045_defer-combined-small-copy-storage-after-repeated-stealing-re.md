+++
schema_version = 1
id = "01M2XHZ7Y5RF1ZSXCFSCK1M7T8"
title = "Defer combined small-copy storage after repeated stealing regressions"
date = "2026-09-19"
status = "proposed"
tags = ["storage"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Deferred; source experiment and all three diagnostics retained
* **Decision**: Do not retain the combined two-entry inline Fragment and copy-memo candidate. Keep the initialized message words and precise Actor roots from Decision167. Allocation savings alone are insufficient to accept an actor throughput optimization.
* **Evidence**: Independent tiny-graph allocation oracles remove 808 bytes of fragment metadata and 108 bytes of memo metadata; the combined tiny copy uses no temporary metadata allocations. The candidate passes the full 2,445-test Rust gate across 295 suites, all native/example/compatibility/fuzz checks, TSan237 with two long churn tests covered normally, and exact actor replay. Yet the [first 144-process diagnostic](../benchmarks/language-comparison/results/actors-parity-small-copy-20260919/STATUS.md) regresses long two-scheduler stealing to 0.752×, and the [144-process repeat](../benchmarks/language-comparison/results/actors-parity-small-copy-confirm-20260919/STATUS.md) regresses it to 0.681×. Preserve both complete runs, frozen source manifests and the exact deferred patch; do not select only improving cells. The repeat improves three of five paired rounds in that cell (median paired ratio1.063); the ratio-of-medians concern does not establish a uniform per-round slowdown or its cause.
* **Baseline check**: A separate [144-process comparison](../benchmarks/language-comparison/results/actors-parity-monitor-baseline-20260919/STATUS.md) between descriptor-cache checkpoint3 and the corrected monitor baseline places eleven of twelve cells within about 5%, with long four-scheduler stealing at 0.885×. It does not reproduce a broad two-to-threefold stage1 regression. Absolute throughput varies substantially across runs without an established cause; no causal environmental explanation or isolated monitor-operation cost is claimed.
* **Next boundary**: Evaluate memo-only and immediately consumed send-result allocations separately, retaining the unchanged workload, reductions and acceptance thresholds. The last full BEAM matrix remains zero of nine strict parity cells. Links and supervision acceptance remain separate from throughput.
