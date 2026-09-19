+++
schema_version = 1
id = "01M2XHZ8PDGGQ1HP7KHTTAK7Y2"
title = "Eliminate eligible self-tail calls without changing cleanup semantics"
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

I will lower direct self calls in return position to parameter updates and a function-local backedge when that function has no owned defer registration. Compiler scratch stack slots will be declared in the entry block, with initialization retained at each logical use.

## Context

Fern uses recursion instead of while/loop. Ordinary native calls grow the stack, and QBE alloc8 outside the entry block can allocate dynamically on repeated paths. Deferred cleanup must still execute once per actual function activation.

## Consequences

Argument expressions evaluate left-to-right into temporary values before any parameter slot changes. Faults and early exits skip later arguments. Full-width values and the existing environment/fault context are preserved. Functions owning defer, mutual recursion and indirect calls keep ordinary calls; nested lifted closure bodies do not disable an otherwise eligible parent. This is direct self-tail-call elimination, not a general proper-tail-call guarantee. Hoisted scratch storage prevents loop and with temporaries from growing the stack on each backedge. The unavailable `/decision` skill is replaced by the established decision format.
