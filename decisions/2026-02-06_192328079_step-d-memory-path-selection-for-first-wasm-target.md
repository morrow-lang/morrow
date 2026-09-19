+++
schema_version = 1
id = "01M2XHZ8YFRWYVWJSD8V015KJ5"
title = "Step D memory-path selection for first WASM target"
date = "2026-02-06"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Accepted
* **Decision**: I will choose Perceus baseline (compiler-inserted `dup/drop` + RC headers) as the default memory path for Fern's first WASM target, keep Boehm bridge as an explicit temporary fallback for bring-up only, and defer WasmGC as a non-default option.
* **Context**: Milestone 7.7 Step D required measured comparison and a concrete default/fallback decision. The repository now has Step A-C primitives and constrained codegen insertion, but no shipping WASM backend yet. Step D measurements were captured via `scripts/compare_memory_paths.py` in `docs/reports/memory-path-comparison-2026-02-06.md`, including native perf snapshot, ownership-op microbenchmark (`fern_dup/drop` vs `fern_rc_dup/drop`), and local WASM/WasmGC feasibility probes.
* **Consequences**: The project advances to actor runtime work with memory-path direction settled for first WASM implementation. Future WASM work should implement backend/toolchain integration against Perceus default, use Boehm bridge only for short-lived bring-up, and re-run the Step D comparison artifact once real WASM binaries are produced.
