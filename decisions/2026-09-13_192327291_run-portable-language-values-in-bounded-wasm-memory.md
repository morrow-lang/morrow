+++
schema_version = 1
id = "01M2XHZ85V4B5APZ10NY082T7H"
title = "Run portable language values in bounded WASM memory"
date = "2026-09-13"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; focused Wasmi, CLI and deterministic stress tests pass

## Decision

Reuse the bounded precise aggregate heap for closures, maps, sets, ranges and unions. Resolve indirect calls through a closed typed dispatch set. Restore roots per loop turn and retain function-owned deferred callbacks in a traced LIFO chain. Propagate language faults through cleanup before reporting a host trap.

## Context

The browser application needs ordinary Fern functions and collections, including captured values and cleanup behavior. A separate reduced application language would undermine shared native/browser code.

## Consequences

The existing managed i64 host-handle ABI, fixed memory ceilings and capability boundary remain. Language faults and failed cleanup run remaining defers; external host fuel exhaustion or stack cancellation still bypass language cleanup. Generic specializations receive distinct export identities. Independent tests include 1,024 seeded ordered-map transitions, 9,000 closure/GC turns and a deterministic cleanup/fuel oracle. Native host capabilities remain target-specific; see `docs/WASM_LANGUAGE.md`.
