+++
schema_version = 1
id = "01M2XHZ84AG2R1SMCMBH9BDZXN"
title = "Compose actor functions through typed returns and ordinary callback identities"
date = "2026-09-14"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; focused native, REPL, inference and independent simulation tests pass
* **Decision**: Infer mailbox effects across lexical direct-call components and refine recursive components before generalization. Permit receiving helpers to return typed values, carrying Result duties through ordinary call summaries. Normalize `with` and `?` into checked control flow, and dispatch ordinary captured callbacks through their original identities into typed actor copies. Give each List callback element an explicit continuation boundary; preserve Option/Result branch selection.
* **Context**: A helper that waits for a value should compose with a caller that handles its Result without a dummy receive or source-order-dependent annotation. A function value should preserve its lexical captures and result ABI while allowing recursive actor work to yield. Treating receiving calls as empty Result provenance would silently discard errors.
* **Consequences**: Spawned initializers still return Unit. Non-tail receiving helpers, strict operands, shared error handlers and collection callbacks preserve full-width values, source evaluation order and logical cleanup under collection. Private list builders stay unpublished until map/filter completion. First-class actor-effect helpers still reject rather than capturing an execution-context pointer. Older REPL closures retain their original checked program through a bounded synchronous fallback; current-program actor callbacks use resumable dispatch. Independent native and interactive models cover branch selection, short-circuit behavior, callback factories, Unicode/Float/Int payloads, sibling progress, cancellation and Result-duty rejection. See `docs/ACTOR_CONTINUATIONS.md` and `docs/REPL_ACTORS.md`.
