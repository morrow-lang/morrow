+++
schema_version = 1
id = "01M2XHZ81VER5RNWAQ5RP2E3NJ"
title = "Report argument type mismatches before missing labels; add sort, zip, range, sum and checked arithmetic"
date = "2026-09-14"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2XHZ9662NTPK8WE6SAVDN4X", "01M2XHZ8R5ZQ6PSV7K2SZ3HR0J"]
+++
## Status

Adopted; independent runtime ABI, native output, REPL and inventory tests pass

## Decision

Check argument types before enforcing required labels so a wrong type is the first report; a call with correct types and a missing label still fails. Add `List.sort` (element-directed: Int/Bool words, Float total order, String bytes, rejecting other elements at check time through a `Sort` capability), `List.zip` (runtime-built compiler-layout pair tuples), `List.range` (half-open, bounded) and `List.sum` (wrapping). Add `Int.checked_add/sub/mul/div/rem/neg` returning `Option(Int)`.

## Context

`add(1, "two")` reported only the missing label ([Decision 7](2026-01-27_192328326_labeled-arguments-for-clarity.md)), hiding the type error. Sorting, pairing, counting and summing required hand-written recursion, and the documented wrapping default ([Decision 55](2026-09-05_192327877_define-numeric-domains-and-unwind-runtime-faults-through-cle.md)) had no checked companion.

## Consequences

Label requirements are unchanged in meaning; only diagnostic order differs. `List.sort` on structured elements was a checker error at this point; [Decision 153](2026-09-14_192327148_sort-structured-elements-through-ord-with-a-runtime-driven-m.md) added ordering through `compare`. Range and zip fault beyond 16,777,216 elements like other allocation limits. The runtime symbol inventory grows to 295 and the lowering audit substitutes tuple return schemes. See `docs/STDLIB_API_REFERENCE.md`.
