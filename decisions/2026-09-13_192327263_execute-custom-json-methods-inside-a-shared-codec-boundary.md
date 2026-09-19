+++
schema_version = 1
id = "01M2XHZ84ZTNE8NTBWKZFQ4SW9"
title = "Execute custom JSON methods inside a shared codec boundary"
date = "2026-09-13"
status = "accepted"
tags = ["architecture"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; focused native, REPL, quota and forced-collection tests pass

## Decision

Make `Json(a)` a statically resolved trait with fallible `to_json` and `from_json` methods. Concrete codec plans retain validated function identities; native descriptors use width-preserving callback thunks and an explicit managed-payload flag. Custom wire shapes are opaque to the conservative union and nullability proof.

## Context

Derived structural codecs cannot express domain-specific encodings such as string-form user identifiers. Running arbitrary trial decoders would create ambiguous branch priority. Independent callback allowances would let nested or caught failures reset a resource budget.

## Consequences

Native and REPL custom methods compose inside structural plans and generic wrappers. Nested JSON operations share work, allocation, node and recursion limits; quota failure in an infallible constructor unwinds cleanup and becomes JSON error 4. Ordinary faults preserve the original fault and unwind callers normally. Explicit error paths compose with their containing JSON pointer. Native callback tests cover scalar widths, full-width integers, managed siblings, forced collection, recursive codecs and recovery after exhaustion. Native callbacks remain ordinary application code, without separate instruction preemption. JSON APIs are not yet part of the portable WASM runtime. See `docs/CUSTOM_JSON.md`.
