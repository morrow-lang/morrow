> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

# Milestone 7.7 Step D Memory Path Comparison (2026-02-06)

## Scope
Compare Boehm bridge vs Perceus baseline vs WasmGC feasibility, then choose default + fallback for first WASM target.

## Measurements (this repository, local run)

### Native perf snapshot
- `bin/fern` size: `514136` bytes
- startup median: `2.07` ms
- startup p95: `4.94` ms
- startup max: `5.95` ms

### Ownership operation microbenchmark
- iterations per run: `5000000`
- Boehm bridge surface (`fern_dup` + `fern_drop`) median cost: `1.2252` ns/iter
- Perceus baseline surface (`fern_rc_dup` + `fern_rc_drop`) median cost: `3.1918` ns/iter
- Perceus/Boehm op-cost ratio: `2.61x`

Interpretation:
- Boehm bridge dup/drop is effectively near-zero semantic overhead (no-op ownership bridge under GC).
- Perceus baseline ops already incur measurable metadata update cost, acceptable for baseline bring-up and expected to improve once borrow/reuse analysis reduces inserted operations.

### WASM/WasmGC feasibility probes
- Fern CLI exposes WASM target: `False`
- Embedded QBE has wasm backend in-tree: `False`
- `emcc` available: `False`
- local clang wasm32 object probe: `False`
- local clang WasmGC flag probe (`-mwasm-gc`): `False`

Current blockers for shipping any WASM memory path from this repo state:
- Fern CLI has no WASM compilation target yet
- Embedded QBE backend has no wasm target directory in-tree
- local clang wasm32 object probe failed

## Decision (Step D)
- default path: **Perceus baseline (compiler-inserted dup/drop + RC headers) for first WASM target**
- fallback path: **Boehm bridge for early bring-up only (non-default, explicitly temporary)**
- WasmGC posture: **defer for first shipping target**

## Go/No-Go criteria for entering WASM implementation
1. Add a compiler-level WASM target surface in Fern CLI/codegen.
2. Add/choose a wasm backend path (QBE extension or alternative backend path).
3. Re-run this comparison including real WASM artifact metrics:
   - wasm binary size,
   - startup latency in runtime host,
   - representative runtime microbenchmarks with dup/drop-heavy workloads.
4. Keep Boehm bridge limited to bring-up/testing; do not make it default once Perceus path is production-ready.
