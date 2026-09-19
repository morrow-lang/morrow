+++
schema_version = 1
id = "01M2XHZ83F5YB7J6HPB9G385BM"
title = "Reuse native callback root registrations without changing immutable values"
date = "2026-09-14"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; independent allocation, native value/ABI and seeded lifetime checks pass
* **Decision**: Store native frames in a reusable vector with nonwrapping monotonic tokens, originating heap identity and stable root ranges. Use push/pop for normal calls; preserve out-of-order and stale-token behavior through a search. Stream only the collected heap's frame ranges into tracing, alongside persistent root registrations. Remove retired heaps' frames before their storage becomes invalid.
* **Context**: Profiling the immutable 256-entry model found repeated insertion/removal in two GC bookkeeping trees dominating callbacks. Most callbacks return an unchanged record, so this cost was paid millions of times without a new record allocation. Repeated callbacks now allocate no bookkeeping after warming to their maximum active depth.
* **Consequences**: The same immutable workload is 2.73–2.94× faster with similar measured RSS. Normal frame operations are amortized O(1), unusual out-of-order exits O(active depth); storage follows peak active depth until invocation shutdown. Lists still map into fresh collections, unchanged records remain shared, actor continuation boundaries and compiler optimization settings are unchanged. Independent retained-version, exact-payload, fault/defer, precise-GC and cross-heap lifetime oracles accompany the change. This is a runtime overhead improvement, not a new collection representation or an actor throughput claim. See `benchmarks/language-comparison/IMMUTABLE.md`.
