+++
schema_version = 1
id = "01M2XHZ8GV7WPR3VGD71SWXYHS"
title = "Preserve signed 64-bit Int through the C frontend ABI"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for the shared runtime migration

## Decision

I will lower checked C-frontend Int values as QBE `l` through literals, parameters, returns, locals, arithmetic, comparison operands, heap Result payloads, tuples, lists and ranges. Bool/Unit retain `w`, Float retains `d`, and actual native C `int` results receive sign extension when exposed as Fern Int.

## Context

Passing a timeout such as 4294967297 through a 32-bit intermediate silently converts an invalid limit into a valid one. Changing only the final runtime call cannot repair values already narrowed in helpers or local arithmetic. Raw function pointers also need their own callable identity rather than managed-pointer cleanup.

## Consequences

Decimal accumulation checks signed bounds; unary MIN is accepted. Add/subtract/multiply/negation wrap, MIN/-1 division yields MIN and remainder zero, and inclusive MAX ranges stop before incrementing. Typed direct/indirect calls and nested Result tuple bindings preserve their native widths. C's legacy packed Option still has a 32-bit payload; power/bitwise and controlled zero-divisor cleanup gaps remain open. This is scoped compatibility work, not full C/Rust parity. The unavailable `/decision` skill is replaced by this established format.
