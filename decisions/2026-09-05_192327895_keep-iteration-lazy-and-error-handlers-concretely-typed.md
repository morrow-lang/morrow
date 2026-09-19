+++
schema_version = 1
id = "01M2XHZ8RQ10JKH2B7CVXDTTA4"
title = "Keep iteration lazy and error handlers concretely typed"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will represent ranges as immutable Int endpoints with an inclusive flag, iterate List/Map/Range values once in their defined order, and give each function its own stack of loop targets. With blocks will retain sequential checked steps and handlers specialized for each distinct error type.
* **Context**: Materializing a full-width integer range would allocate unbounded memory or overflow at an inclusive maximum endpoint. Fern specifies heterogeneous errors in with blocks; forcing them into one inferred error type would reject the documented control flow. Repeatedly nesting source-level matches would also turn a flat block into deep compiler recursion.
* **Consequences**: Empty or reversed ranges produce no iterations. Inclusive ranges test their final value before incrementing. Break and continue target the nearest loop in the same function and leave deferred cleanup registered until function exit. Map iteration follows insertion order and yields key/value tuples; list enumeration yields index/value tuples. With steps stop at the first error, each concrete handler preserves applicable source-arm order and requires exhaustiveness, and successful binders are unavailable in error handlers. Explicit error arms use Err(pattern) or an unbound wildcard; named catches use Err(name). Without else, errors propagate under the same enclosing Result constraint as postfix ?. No erased or fabricated Result payload types are introduced. The unavailable `/decision` skill is replaced by the established decision format.
