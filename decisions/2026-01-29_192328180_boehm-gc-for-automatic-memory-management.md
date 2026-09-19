+++
schema_version = 1
id = "01M2XHZ91MWCZ6FPXT178RZ4RW"
title = "Boehm GC for automatic memory management"
date = "2026-01-29"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Adopted
* **Decision**: I will use Boehm GC for automatic garbage collection in Fern programs, with a future path to BEAM-style per-process heaps when actors are implemented.
* **Context**: Fern's immutable-first, functional style generates many intermediate values (strings, lists, etc.) that need automatic memory management. Considered several approaches: (1) Manual memory management - error-prone, leaks inevitable, (2) Reference counting - works but has cycles problem and overhead, (3) Boehm GC - conservative, drop-in replacement for malloc, proven in production, (4) Custom tracing GC - complex, takes months to implement well, (5) BEAM-style per-process heaps - ideal for actors but requires actor runtime first. Chose Boehm GC as the pragmatic v1 solution: ~100 lines of integration, zero memory leaks, works with C FFI. When actors are added (Milestone 8), we'll transition to per-process heaps where each actor has its own GC'd heap - this eliminates global GC pauses and enables instant memory reclamation on process death.
* **Consequences**: Runtime uses `GC_MALLOC` instead of `malloc`. All `_free()` functions become no-ops. Compiled programs link with `-lgc`. Requires `brew install bdw-gc` (macOS) or `apt install libgc-dev` (Linux). Binary size increased ~20KB. No measurable performance impact in benchmarks. Future actor runtime will use per-process heaps with generational collection within each process.
