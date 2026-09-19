+++
schema_version = 1
id = "01M2XHZ8QM7TCJAQR5VSAEPE9E"
title = "Preserve UTF-8 strings at slicing and splitting boundaries"
date = "2026-09-05"
status = "accepted"
tags = ["architecture"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for Rust migration completion

## Decision

I will retain byte-indexed String.slice and its existing index clamping, while requiring clamped endpoints to be Unicode scalar boundaries. Splitting on an empty delimiter produces one complete Unicode scalar per String.

## Context

Native byte-by-byte splitting can produce invalid UTF-8, while interactive strings already reject invalid slices. DESIGN defines byte lengths without specifying these non-ASCII corner cases. A String must remain valid UTF-8 across these operations.

## Consequences

Clamping first sets start to at least zero and end to at least start, then bounds both by byte length. Interior-byte endpoints are errors even when the requested slice is empty. Rust guards execute deferred cleanup before reporting `String.slice indices must be UTF-8 character boundaries`; the shared legacy C function rejects the same request independently. Empty input split on an empty delimiter yields an empty list; combining marks remain separate scalars, with no implicit grapheme segmentation or normalization. Nonempty delimiter behavior is preserved. The unavailable `/decision` skill is replaced by the established decision format.
