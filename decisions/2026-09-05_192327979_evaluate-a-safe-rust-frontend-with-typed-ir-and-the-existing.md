+++
schema_version = 1
id = "01M2XHZ8VBJ09WNWNV74SD6TAR"
title = "Evaluate a safe Rust frontend with typed IR and the existing native backend"
date = "2026-09-05"
status = "accepted"
tags = ["rust"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted experiment; shipping C frontend remains default
* **Decision**: I will implement an independent, dependency-free Rust 2021 frontend prototype in `compiler-rs`, carry resolved types and symbol IDs through a typed IR, and reuse the vendored QBE backend and C runtime through an isolated backend process. I will compare the supported subset against specification-grounded native-output fixtures and the C compiler before recommending broader migration.
* **Context**: The user authorized a measured Rust migration experiment, superseding decision 2's C-only restriction for this prototype. Recent type reconstruction and pointer-lifetime bugs justify evaluating stronger implementation guarantees without replacing working native features. The `/decision` skill is unavailable; the established decision format is used directly.
* **Consequences**: Rust uses standard owned types, enums, Vec, and Result; C-specific Datatype99/SDS/arena rules remain applicable to C. Safe Rust is required (`forbid(unsafe_code)`), runtime behavior and Fern syntax do not change, unsupported prototype constructs produce explicit diagnostics, and the old compiler remains available. Bounds and parser depth are enforced; idiomatic type-enforced invariants replace redundant Rust assertions. A small process boundary isolates QBE's global state/abort behavior and avoids Rust FFI ownership hazards. Benchmark frontend work separately from shared backend/link work.
