+++
schema_version = 1
id = "01M2XHZ84HK3AZWYQ7R6YBJWH0"
title = "Keep actor cleanup attached to logical function activations"
date = "2026-09-13"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; native and REPL lifecycle tests and deterministic scope simulation pass
* **Decision**: Retain a traced cleanup stack per actor. Enter and leave logical source-function scopes across physical callback suspension, register captured Unit cleanup adapters in LIFO order, and drain all remaining scopes during fault retirement or explicit cancellation. Keep cleanup invocation synchronous and reject actor suspension from deferred bodies.
* **Context**: A physical callback may finish while its source function is still waiting for a message. Running `defer` at that callback return would release resources too early; omitting it on cancellation would leak source-level resource lifetimes.
* **Consequences**: Tail callers retain pending cleanup until the callee returns. Return values and deferred captures remain rooted through collection. Cleanup failures do not skip later callbacks and never replace an earlier source fault. Scopes and callbacks share a 4,096-entry admission limit, with retained-byte accounting released on retirement. Independent native and REPL oracles verify receive/return/cancel/fault order, precise collection, admission failure and recovery. A 64-by-256-operation simulation compares runtime scopes against a separate stack model. Blocking or nonterminating native cleanup still requires application discipline; this is not arbitrary instruction preemption.
