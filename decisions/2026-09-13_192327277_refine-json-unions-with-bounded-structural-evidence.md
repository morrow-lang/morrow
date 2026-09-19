+++
schema_version = 1
id = "01M2XHZ85DSD35WF671TSTWFBX"
title = "Refine JSON unions with bounded structural evidence"
date = "2026-09-13"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; focused compiler, REPL, native and precise-GC tests pass
* **Decision**: Extend the shared symbolic/concrete wire proof with tuple elements, required record-field children and shared-tag sum payloads. A finite incompatible child proves the alternatives disjoint; recursive pairs are conservatively unresolved. Keep kind and shallow selection first, then inspect borrowed nested shapes only when several candidates remain. Decode the unique selected member once.
* **Context**: Same-key records containing numbers versus strings were rejected even though their wire domains cannot overlap. Trial decoding would introduce arm priority, speculative allocation and inconsistent error reporting. Structural metadata can distinguish these records without executing a decoder.
* **Consequences**: The original aggregate proof and runtime work/depth allowances cover nested traversal. Two optional fields may both be absent; Int/Float, empty Lists and dynamic JSON retain their overlap rules. Existing primitive conversion errors remain unchanged after a unique selection. Every shared sum tag must have disjoint payloads; a common empty constructor remains ambiguous. Independent seeded REPL/native wire oracles cover full-width integers and Unicode; native tests prove allocation-free selection, collection-safe construction and reclamation. Obsolete Int/String record rejection cases now test actual Int/Float overlap, alongside new positive behavior tests.
