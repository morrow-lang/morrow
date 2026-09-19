+++
schema_version = 1
id = "01M2XHZ90E7ZST9KXFCDYB92QE"
title = "Perceus-style reference counting for WASM support"
date = "2026-01-29"
status = "superseded"
tags = []
supersedes = []
superseded_by = ["01M2XHZ904FEPHWC820GCZA93V"]
depends_on = []
related_to = []
+++
* **Status**: 🔄 Superseded by [27]
* **Decision**: I will implement Perceus-style compile-time reference counting as Fern's long-term memory management strategy, replacing Boehm GC for both native and WASM targets.
* **Context**: Boehm GC works well for native targets but cannot support WASM (relies on stack scanning and OS features). Considered several alternatives: (1) Rust-style ownership - powerful but steep learning curve, (2) Swift ARC - requires manual weak references for cycles, (3) WasmGC - ties us to browser GC, may have pauses, (4) Perceus (from Koka/Roc) - reference counting with reuse optimization. Chose Perceus because: functional purity eliminates cycles (no weak refs needed), zero developer annotations required, works identically on native and WASM, no GC pauses, and enables "functional but in-place" optimization where unique values are mutated behind the scenes.
* **Consequences**: Created `docs/MEMORY_MANAGEMENT.md` with detailed design. Implementation in phases: (1) Keep Boehm for now, (2) Add Perceus for WASM target, (3) Replace Boehm everywhere, (4) Add reuse optimization. Compiler will insert dup/drop operations automatically. Developers write pure functional code; compiler figures out optimal memory strategy.
