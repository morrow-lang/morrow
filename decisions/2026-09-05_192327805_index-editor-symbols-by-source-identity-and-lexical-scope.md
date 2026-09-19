+++
schema_version = 1
id = "01M2XHZ8NX9ZQZNFQRZKBXH2HB"
title = "Index editor symbols by source identity and lexical scope"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for Rust migration completion

## Decision

I will build bounded editor symbol snapshots from the current source graph and lexical bindings, with definitions identified by their original file and source anchor. Navigation and completion will use the same module visibility and qualification facts as compilation.

## Context

Final IR identifiers are local to functions and may be duplicated by generic specialization or closure lifting. Text matching cannot distinguish shadowed bindings, separate clause parameters or imported private names. Existing unsaved overlays and UTF-16 synchronization already define coherent editor inputs.

## Consequences

Accepted edits invalidate semantic snapshots. Unresolved or invalid current source produces no stale semantic locations. Completion is deterministic, bounded and respects scopes, aliases and public exports; builtin prefix completion may remain available on incomplete source without inventing types. Exact source ranges distinguish code from comments and literal text. Semantic hover and typed members require checker facts and will be advertised only when implemented. The unavailable `/decision` skill is replaced by the established decision format.
