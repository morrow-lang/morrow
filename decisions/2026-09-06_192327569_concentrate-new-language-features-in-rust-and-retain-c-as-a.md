+++
schema_version = 1
id = "01M2XHZ8EHT3630TCTPM7ZZDN6"
title = "Concentrate new language features in Rust and retain C as a bootstrap reference"
date = "2026-09-06"
status = "accepted"
tags = ["rust"]
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2XHZ8VBJ09WNWNV74SD6TAR", "01M2XHZ8V2ZQC09CYEJGVQZRB6"]
+++
## Status

Accepted migration direction; default command migration remains unverified

## Decision

I will complete new source-language features in Rust, then retain the C frontend as an explicitly selected bootstrap/reference executable after the Rust default passes its migration gates. I will preserve tested legacy native ABI symbols without duplicating every new source feature in C.

## Context

Decisions [45](2026-09-05_192327979_evaluate-a-safe-rust-frontend-with-typed-ir-and-the-existing.md)/[46](2026-09-05_192327970_extend-rust-through-typed-collections-and-built-in-sum-types.md) retain C as the shipping default during validation, not as a permanent second implementation. The JSON audit reproduces invalid text accepted by the legacy copy API, all ten new JSON fixtures failing at qualified type syntax, and absent C Map code generation. Porting the whole new API back would duplicate Rust work and prolong two divergent implementations.

## Consequences

C remains the current default until command/API compatibility, native execution, tooling, packaging and platform gates justify an explicit switch. The switch must document executable selection and intentional source differences, including JSON, while preserving bootstrap workflows and legacy ABI regressions. Retiring the legacy source API is not evidence that typed JSON codecs, remaining standard modules or full language semantics are implemented; those remain Rust completion requirements. No current executable is renamed or removed by this decision. The unavailable `/decision` skill is replaced by this established format.
