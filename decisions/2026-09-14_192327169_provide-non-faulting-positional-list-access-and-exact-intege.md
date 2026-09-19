+++
schema_version = 1
id = "01M2XHZ8212KTVZ3PBQX8TK02Q"
title = "Provide non-faulting positional list access and exact integer parsing"
date = "2026-09-14"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; independent runtime ABI, native output, REPL and inventory tests pass

## Decision

Add `List.at`, `List.first`, `List.last` returning full-width heap `Option(a)`, `List.take`/`List.drop` clamping their count into `0..=len`, and `Int.parse` accepting exactly `[+-]?[0-9]+` within i64 range as `Option(Int)`. Keep `List.get` and `List.head` as the faulting forms.

## Context

The only positional accessors faulted on invalid positions, and no source API converted text to an integer, so ordinary programs either faulted at runtime or pattern-matched around `List.is_empty` and `String.is_decimal`. Fern's error model requires such absence to be a visible `Option`, not a process fault.

## Consequences

The runtime encodes `None` as `Err(0)` and `Some` as `Ok(word)` through the existing heap Result helpers, so payloads such as `Int` minimum survive. Negative and oversized counts never wrap. `Int.parse` deliberately rejects whitespace, separators, radix prefixes, exponents and non-ASCII digits; callers normalize text first. The browser target keeps its existing limited runtime surface. See `docs/STDLIB_API_REFERENCE.md`.
