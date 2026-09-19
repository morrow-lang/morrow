+++
schema_version = 1
id = "01M2XHZ877KW7BTEDY2E5S0ME6"
title = "Reuse actor slots and suspend recursive Unit-tail paths"
date = "2026-09-13"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; independent runtime and native progress oracles pass
* **Decision**: Separate reusable actor-table slots from immutable nonwrapping u64 generations. Retain exact dead control identities while PIDs reference them, including PID wrappers in other actor heaps, without retaining dead actor payloads. Compile eligible recursive Unit-returning tail paths into separate resumable actor callbacks while preserving ordinary function/closure calls and the no-actor CLI path.
* **Context**: A 65,536-identity lifetime cap stopped otherwise bounded long-running sessions. Reusing an old control object could revive a stale PID; tracing only the invocation heap could instead free a control identity still referenced by another actor. Explicit PID-to-control metadata edges preserve the immutable identity until the wrapper is swept or its heap retires. Separately, counting callbacks did not interrupt a recursive helper that never returned to the scheduler.
* **Consequences**: Actor completion releases active logical storage and its occupied slot. Old supervision handles resolve their retained lineage without redirecting sends to replacement actors. Churn beyond the former cap, generation exhaustion, stale sends, foreign roots and eventual reclamation have independent tests. Recursive Unit-tail paths through supported blocks, branches, matches and receive arms/timeouts can hand off between callbacks with copied, rooted arguments; native tests cover aliases, mutual recursion, full-width integers and collection at every handoff. Finite helpers and ordinary calls keep their prior behavior. Non-tail/numeric recursion, collection loops, deferred cleanup, `with` and unsupported capture types remain synchronous. Collection and copying are bounded but not yet resumable or charged as instruction work; this is not general preemption.
