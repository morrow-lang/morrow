+++
schema_version = 1
id = "01M2XHZ8382NWRCQQ9PW7YM46C"
title = "Optimize native list callbacks under explicit scope and allocation proofs"
date = "2026-09-14"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2XHZ83F5YB7J6HPB9G385BM"]
+++
## Status

Adopted; staged measurements and independent native, GC, fault and fallback regressions pass

## Decision

Emit native code with Cranelift `opt_level=speed`, retaining frame pointers, verifier and conservative memory flags. Validate list bounds once and directly traverse rooted, nonmoving payload storage. Populate only fresh, capacity-proven map/filter builders. Inline statically known native list callbacks with scope-neutral bodies bounded to 64 expression nodes and 64 parameters/captures, using the evaluated environment and enclosing physical root/fault frame. Preserve indirect fallback for dynamic, large, explicit-call, cleanup/control and actor callbacks.

## Context

Backend optimization alone barely changes the immutable workload while per-element runtime calls remain. Direct list access improves it about 5–6%; callback inlining supplies the largest gain. Once calls are removed, backend optimization adds another 6–7%. The paired final workload is 2.98–3.71× faster than [Decision 146](2026-09-14_192327215_reuse-native-callback-root-registrations-without-changing-im.md), with similar RSS; scalar performance is unchanged and small-source builds grow from about 43 to 45 ms.

## Consequences

Source immutability, full-width payloads, callback evaluation order, arithmetic faults and precise roots remain required. General indexing keeps its checked helper; actor continuation boundaries, ordinary calls and Option/Result callback lowering are unchanged. Normal emitter/root budgets remain enforced and excluded bodies use their existing callback activation. Tests prove actual inlining, retained historical versions, precise collection during allocations, short-circuit behavior, fault/defer cleanup and bounded fallback. See `benchmarks/language-comparison/NATIVE_OPTIMIZATION.md`; these measurements do not claim a new collection representation or actor throughput.
