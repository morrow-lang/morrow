+++
schema_version = 1
id = "01M2XHZ81CWVEEDQTHEVC1PV19"
title = "Sort structured elements through Ord with a runtime-driven merge; show strings as literals"
date = "2026-09-14"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2XHZ81VER5RNWAQ5RP2E3NJ", "01M2XHZ81KCTK6SK4AKQWQX9NM"]
+++
## Status

Adopted; independent runtime state-machine, native output, REPL, wasm and checker tests pass

## Decision

Add the higher-order builtin `List.sort_by(items, compare: (a, a) -> Ordering)`. `List.sort` keeps the runtime scalar orders for `Int`, `Float`, `Bool`, `String` and newtypes over them; for every other element type, including generic parameters, the checker rewrites it to `List.sort_by(items, compare)` using the `Ord` trait method, activating the trait prelude on demand like `println`. Sorting is a stable bottom-up merge sort whose control state lives in a collector-managed runtime object (`fern_sort_begin/next/report/finish`): the runtime chooses which positions to compare, compiled code performs each comparison through the typed closure and reports the `Ordering` tag, and the runtime materializes the permutation. No C callback crosses the runtime boundary. `Show(String)` now renders a Fern string literal (`String.quote`: quotes plus `\" \\ \n \r \t` escapes) natively, in the REPL and in the browser target, and `Ord(Float)` joins the prelude with NaN comparing `Equal`. `println` of a generic parameter routes through `Show` unless the program defines its own `show`.

## Context

[Decision 151](2026-09-14_192327163_report-argument-type-mismatches-before-missing-labels-add-so.md) rejected structured elements at check time and [Decision 152](2026-09-14_192327155_print-structured-values-through-show-with-an-on-demand-prelu.md) left generic bodies scalar-only. Writing a merge sort in Fern source costs quadratic copying on array lists, and the higher-order lowering invariant forbids native callbacks. `println(["a", ""])` printed `[a, ]`, hiding empty and spaced strings.

## Consequences

`derive(Ord)` types, tuples, options and lists sort with the same lexicographic order as `compare`. `List.sort_by` needs 316 native fixtures, the runtime inventory grows to 300 symbols, and the browser target reports `List.sort_by` as an unavailable host capability. Result-bearing elements are rejected by the obligation proof rather than given invented positions; the comparator itself is proven once over two general elements. Programs that rely on the old raw string rendering must update expected output.
