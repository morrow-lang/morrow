+++
schema_version = 1
id = "01M2XHZ8PMKWK6TZNCBCEG1T8Y"
title = "Normalize adjacent function clauses through the shared pattern engine"
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

I will retain adjacent clauses in source syntax and normalize each group to one checked function before return inference. Typed pattern parameters, guards and arrow bodies use the existing exhaustive match semantics.

## Context

DESIGN specifies function clauses and pattern parameters, while the Rust frontend already has shared pattern coverage, function-owned control flow and cleanup. A separate dispatch implementation would risk different coverage and Result handling rules.

## Consequences

Clauses must agree on arity, parameter types, visibility and supplied return annotations; initially generic names must remain consistent across a group. Guards do not guarantee coverage, and missing cases are errors rather than DESIGN's earlier warning. Whole-function documentation appears before the first clause. Synthetic argument names cannot collide with source identifiers; balanced dispatch tuples preserve the 255-parameter limit. Whole-pattern Result discard checks precede hidden argument reads, and each arm retains its own binder obligations. This checkpoint requires annotated parameter patterns; pattern-anchored inference and complete private signature generalization follow separately. The unavailable `/decision` skill is replaced by the established decision format.
