+++
schema_version = 1
id = "01M2XHZ8BYB69321D49Y11S2RP"
title = "Incremental Rust lint and benchmark guidance"
date = "2026-09-06"
status = "accepted"
tags = ["rust", "performance"]
supersedes = []
superseded_by = []
depends_on = ["01M2XHZ8C5CX1RJ31TATKP1D95"]
related_to = ["01M2XHZ8BQVVV17FRV09VJM2NS"]
+++
## Decision

I will enforce tested MSRV-compatible lints, audit strict restrictions module by module, and measure compiler phases in an independently locked Criterion developer package.

## Context

The attached review guidance calls for practical safety checks and developer tools while preserving the existing project contracts.

## Consequences

Adopt eight MSRV-compatible package Clippy restrictions, production panic restrictions,

and a stricter audited source-directory module. Test lint names and enforcement with
real offline negative crates. Keep validated bounded arithmetic narrowly documented.
Reject oversized initial source paths before allocation/filesystem lookup. Later audited native linker and frame modules receive the same restrictions. Preserve nonbreaking-space path bytes with ASCII shell delimiters; reject NUL and over-budget linker records/words/argument counts before parser allocation. Checked frame access/conversion preserves the existing protocol and lifecycle, including all 256 exit codes; no frame semantic defect was found. The pkg-config library-directory record has a separate borrowed-path decoder: preserve literal whitespace and remove only one LF/CRLF terminator; reject empty, non-UTF-8, NUL, multiline or over-4096-byte paths before archive lookup. Malformed successful metadata must not select a lossy/trimmed/current-directory decoy or silently use the --libs fallback. Unavailable or unsuccessful pkg-config retains the existing fallback.

Add Criterion 0.5.1 in an independent unpublished developer workspace with an exact
Rust1.75-tested dependency lock. Measure parsing, checking, QBE emission and actual
codec validation, with independently verified fixtures and black_box. Keep compiler
production dependencies and lock unchanged. CI smoke checks behavior; statistical
baselines are optional and cannot alone establish performance improvement.

Use the [Decision 106](2026-09-06_192327493_reproducible-development-tasks-with-mise.md) mise workflow and its tested optional tools, not Nix/devenv or
a new Justfile. Preserve the production MSRV and exact CLI semantics ([Decision 108](2026-09-06_192327479_preserve-common-cli-controls-during-rust-migration.md)
owns any CLI parser change). docs/RUST_GUIDANCE.md records every attached suggestion,
its adoption/omission rationale and remaining boundary. No unrelated product crate,
global tool installation, native runtime changes, or automatic dependency updates.
