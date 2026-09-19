+++
schema_version = 1
id = "01M2XHZ889SVS560KRJ6YHDSZK"
title = "Prioritize supervised native actors and reactive Fern WebAssembly applications"
date = "2026-09-12"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted architecture; implementation gates remain open

## Decision

Build toward an integrated Elixir/Phoenix alternative: native server actors, a reactive Fern browser application compiled to WebAssembly, and shared typed WebSocket protocols. Keep ordinary Fern memory management automatic. Prioritize actor-owned tracing heaps, precise roots, copied messages, fair resumable scheduling and typed supervision. Begin browser work with a separate wasm32 ABI and precise linear-memory tracing; evaluate WasmGC before stabilizing that ABI. Inferred ownership, borrowing and reference-counted immutable buffers remain implementation optimizations, not mandatory application-language concepts.

## Context

The user explicitly selects the full-stack actor/browser direction. Current actors share an invocation-thread heap, retain message payloads and run cooperatively; those properties do not establish multicore isolation or a long-running server. Native stack/register scanning and LP64 layouts also cannot simply become a browser runtime. This workload prioritizes isolation and latency accounting over a universal Perceus migration.

## Consequences

The [full-stack architecture](../docs/FULL_STACK_ARCHITECTURE.md) defines authority, memory, fairness, browser bindings, wire compatibility, reconnect/idempotency, overload, supervision and staged scaling. Compiler/runtime/host/tooling remain Rust-authored; application logic is Fern, with generated browser JavaScript interop allowed only as build output. Tree-sitter stays removed. A first-party web/UI framework is separate from mandatory language primitives and CLI dependencies. The first integration target is a bounded, explicitly ephemeral two-browser collaborative checklist; production durability and multicore/multi-node guarantees have separate gates. This decision supersedes any assumption that one reference-counting collector must serve every target, and prioritizes first-party web/UI work over the earlier ecosystem-only framework boundary. It does not mark WASM, HTTP serving, actor isolation or Phoenix parity as implemented.
