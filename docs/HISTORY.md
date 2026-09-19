> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

# Fern History

This file is a curated, non-actionable history summary. For active work, use [`../ROADMAP.md`](../ROADMAP.md).

## Timeline

- 2026-02-06: Gate A (DX), Gate B (reliability), Gate C (stdlib/runtime), and Gate D (ecosystem/adoption) all marked passed.
- 2026-02-06: Runtime behavior stabilized for `json`, `http`, `sql`, and actor supervision baseline paths.
- 2026-02-06: Memory path decision recorded for first WASM target (Perceus baseline default, Boehm bridge fallback).
- 2026-02-06: Release automation and benchmark publication paths integrated in CI.

## Primary Historical Sources

- Decision records: [`../decisions/`](../decisions/)
- Benchmark artifact: [`reports/benchmark-case-studies-2026-02-06.md`](reports/benchmark-case-studies-2026-02-06.md)
- Memory comparison artifact: [`reports/memory-path-comparison-2026-02-06.md`](reports/memory-path-comparison-2026-02-06.md)
- Frozen legacy archive pointer: [`ROADMAP_ARCHIVE_2026-02-06.md`](history/ROADMAP_ARCHIVE_2026-02-06.md)

- 2026-09-12: Fern-owned implementation moved to a Rust Cargo workspace. Cranelift replaced QBE, the native runtime and supervisor were rewritten in Rust, Rust xtask replaced legacy scripts, and Tree-sitter was removed while retaining the Rust LSP. Earlier implementation reports are archived under `history/`.
