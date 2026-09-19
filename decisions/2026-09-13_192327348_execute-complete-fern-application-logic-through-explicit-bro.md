+++
schema_version = 1
id = "01M2XHZ87M861ZS6BSP8P6753S"
title = "Execute complete Fern application logic through explicit browser and native host boundaries"
date = "2026-09-13"
status = "accepted"
tags = ["architecture"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; focused native, WebSocket and browser acceptance passes, final integrated gates recorded in the roadmap
* **Decision**: Move the shared checklist domain, immutable model, event update and keyed view into ordinary typed Fern. Extend the separate WASM backend to bounded aggregates with precise pointer layouts and compiler roots. Export managed browser values through positive i64/BigInt handles with checked types and nonwrapping generations; transfer UTF-8 through a bounded scratch region. Rust retains generic DOM, storage and transport capabilities. Compile the native actor adapter to a Cranelift object at build time and link it into the Rust server without a compiler, interpreter or Fern process-startup wrapper.
* **Context**: Executing scalar policy while Rust owned application state did not fulfill the full-stack application boundary. A long-lived host also cannot retain naked Fern pointers across collection or pretend a synchronous native call is an async actor. Native library exports therefore take an explicit fault cell and execution context. Open sessions retain their descriptor/control roots, host PIDs have stable registered roots, and typed String ports copy replies into bounded Rust-owned buffers. A dedicated server owner thread constructs and destroys the native domain without Send implementations for its heaps. Supervised typed actors restart from copied initializer captures within a bounded lifetime budget; stale PIDs remain invalid.
* **Consequences**: The checklist now executes its complete application logic in Fern on both sides. Browser managed handles use 55-bit generations after review found that first-free i32 handle reuse could exhaust in an ordinary long-running UI. Precise nested-value and destructuring-root pressure tests protect that ABI. Native room tests cover isolated state, Unicode, full-width values, fault recovery and 5,000 requests with precise host-root collection. Runtime HTTP and SQLite dependencies are optional and remain enabled by default; the embedded web application uses the core runtime without those unused dependencies. General helper suspension, fair multicore scheduling, full precise native layout coverage, shared wire-schema generation and application-independent framework packaging remain open. These additions do not imply a language 1.0 or Erlang/Phoenix parity.
