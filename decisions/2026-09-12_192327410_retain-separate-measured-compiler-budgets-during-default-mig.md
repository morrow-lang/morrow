+++
schema_version = 1
id = "01M2XHZ89JX7FHQAB9VN5EY3AV"
title = "Retain separate measured compiler budgets during default migration"
date = "2026-09-12"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted; both platform release budgets verified

## Decision

I will retain the C reference compiler's 1,500,000-byte ceiling and require the expanded Rust default to fit 4 MiB, with the existing 150-second build and 100-ms startup-p95 ceilings unchanged. Use ThinLTO and one codegen unit with normal release optimization.

## Context

The full typed frontend, Result proof engine, native testing, editor tools and terminal editor measured about 4.3 MiB with stock release settings, 3.6 MiB with ThinLTO, and 3.1 MiB with size optimization plus ThinLTO on macOS arm64. The original compiler-size ceiling measured the narrower C compiler, not generated Fern applications. Removing implemented language guarantees to match that earlier compiler would defeat the migration.

## Consequences

Both compiler budgets remain enforced independently by mise run perf-budget. Prefer normal optimization over the smaller size-optimized build; measure actual frontend/native workflows and validate both platform releases before promotion. These compiler budgets make no new claim about generated-program size, static linking, or universal performance. Cargo source dependencies and native components remain visible in the release and notices.
