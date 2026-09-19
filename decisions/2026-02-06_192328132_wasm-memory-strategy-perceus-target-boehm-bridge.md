+++
schema_version = 1
id = "01M2XHZ904FEPHWC820GCZA93V"
title = "WASM memory strategy: Perceus target, Boehm bridge"
date = "2026-02-06"
status = "accepted"
tags = ["runtime"]
supersedes = ["01M2XHZ90E7ZST9KXFCDYB92QE"]
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will keep Boehm GC as the shipping memory system for native in the short term, use it only as an optional bridge for early WASM bring-up, and keep Perceus-style compile-time reference counting as Fern's long-term memory model for both native and WASM.

## Context

[Decision 26](2026-01-29_192328142_perceus-style-reference-counting-for-wasm-support.md) assumed Boehm GC could not support WASM. Upstream Boehm now includes explicit WebAssembly (`WEBASSEMBLY`) support paths for Emscripten and WASI, but with practical constraints (notably wasm32 assumptions and limited threading support). Fern still needs deterministic memory behavior, predictable pauses, and a unified actor-friendly model, which align better with Perceus. We also need an incremental path that does not block Gate C work.

## Consequences

Milestone 7.7 becomes a concrete engineering spike with exit criteria (prototype both Boehm-on-WASM and Perceus runtime shape, compare pause behavior, binary size, and implementation risk). [Decision 26](2026-01-29_192328142_perceus-style-reference-counting-for-wasm-support.md) is superseded for the "Boehm cannot support WASM" claim, but Perceus remains the preferred end-state.
