+++
schema_version = 1
id = "01M2XHZ8RY2GTJRQ954DRFEHZT"
title = "Preserve abrupt control flow and function-exit cleanup"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for Rust migration completion

## Decision

I will represent early termination explicitly through checking, typed IR and native emission. Deferred expressions become captured zero-argument Unit closures registered dynamically on a function-owned LIFO stack. Every normal return, explicit return and propagated Result error saves its value before draining that stack.

## Context

Conditionals and match arms can leave a function without producing a value for their enclosing expression. Fabricated operands would execute skipped effects or create invalid native joins. The dedicated DESIGN cleanup section requires function-exit semantics, including registrations in inner blocks, rather than lexical-block cleanup.

## Consequences

Only live control-flow predecessors contribute values. Let-else binds success values into the following scope and requires its failure branch to diverge. Deferred expressions capture immutable lexical values at registration but evaluate their code, including call arguments, at function exit. Cleanup must produce Unit and cannot return or propagate errors; a mandatory cleanup closure may handle captured Results. User lambdas have independent return and cleanup contexts. Interactive evaluation uses a separate bounded cleanup work budget so ordinary evaluation failures can still attempt cleanup while preserving the original failure. Actor cleanup remains outside this contract. The unavailable `/decision` skill is replaced by the established decision format.
