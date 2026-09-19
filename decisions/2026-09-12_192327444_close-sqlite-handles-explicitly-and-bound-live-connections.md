+++
schema_version = 1
id = "01M2XHZ8AMYVPRGXMXC2HPSWWN"
title = "Close SQLite handles explicitly and bound live connections"
date = "2026-09-12"
status = "accepted"
tags = ["storage"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for the shared native runtime and both compiler frontends

## Decision

I will expose `sql.close(handle) -> Result(Int, Int)`, bound live connections at 256, and reuse storage without reusing handle identities.

## Context

Native programs could open and execute SQLite statements but could not release connections or locks. An ever-growing handle table does not provide a usable lifecycle for long-running programs. Keeping monotonically assigned IDs prevents a stale handle from accessing a later connection.

## Consequences

Successful close returns `Ok(0)`; unknown, closed, or invalid handles return the existing IO error. A failed SQLite close retains the connection for retry. Live quota or ID exhaustion returns the existing out-of-memory error before opening a database. Close rolls back outstanding transactions according to SQLite semantics. Both C and Rust native paths share the implementation; SQL remains explicitly unavailable in the REPL. Debug/release/sanitizer and source-output tests cover quotas, recycling, stale handles, transactions and lock release. Typed query APIs remain separate work.
