+++
schema_version = 1
id = "01M2XHZ7YQCS140KS27WVVZJWT"
title = "Cache immutable actor descriptors with unchanged work accounting"
date = "2026-09-19"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted measured cache checkpoint; strict parity remains zero of nine cells
* **Decision**: Give each scheduler an eight-entry descriptor lookup cache keyed by exact registered identity. Preserve each descriptor's original registry-position work charge, including the exact exhaustion point; misses and collisions retain bounded lookup. Cache storage starts empty per Session and is included in physical control accounting; the historical logical Session charge remains unchanged. Explicit C layout on cache records preserves the external Exec declaration. No payload ownership, callback, migration or message-admission semantics change.
* **Independent oracles**: After warming a descriptor at registry index 31, 32 real callbacks reduce measured table comparisons from 4,096 to zero while preserving callbacks and work charges. Tests cover collisions, unknown/null identities, exact budget boundaries, independent registrations and owner migration. Native ABI checks exercise an external Exec declaration rather than suppressing FFI diagnostics.
* **Measurement**: The [144-run quiet diagnostic](../benchmarks/language-comparison/results/actors-parity-descriptor-cache-20260919/STATUS.md) passes every independent output and improves all twelve short/long request/reply cells by 1.022–1.221× over checkpoint2. This comparison does not establish BEAM parity.
* **Acceptance**: Final cache-only `cargo xtask check` passes 2,397 Rust tests across 292 result suites, 317 native fixtures, 20 examples, 63 compatibility programs, 295 atomic rejections and 64+192+231 fuzz cases. The runtime suite has 203 tests; ThreadSanitizer passes 201 in 13.66 s, excluding the same two long churn tests covered normally. Replay retains trace `0xd4e402a412f11e2f`, 48,739 callbacks and zero cleanup residue. The [third matrix](../benchmarks/language-comparison/results/actors-parity-checkpoint3-20260919/PARITY.md) preserves 36 semantic smoke checks and 216 observations: request/reply reaches 0.073–0.308× BEAM, contention 0.273–0.532×, lifecycle 0.096–0.434×. All nine strict cells and confirmation remain open; four-scheduler lifecycle regresses against the previous separate snapshot.
* **Rejected candidate**: Sparse fragment metadata insertion reduced an adversarial tree-comparison oracle from 32,774 to 38 and passed correctness, sanitizer and replay checks. Its [first diagnostic](../benchmarks/language-comparison/results/actors-parity-fragment-adoption-20260919/STATUS.md) and [repeat](../benchmarks/language-comparison/results/actors-parity-fragment-confirm-20260919/STATUS.md) showed inconsistent throughput, including 0.662× short S2 stealing in the first run and 0.777× short S2 pinned/0.807× long S2 stealing in the repeat. Preserve all raw runs and the candidate patch, but keep the existing fragment adoption implementation until a reliable gain is demonstrated. A bounded operation-count improvement alone is not enough to accept a throughput optimization.
