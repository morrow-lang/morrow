+++
schema_version = 1
id = "01M2XHZ8G5MRNKGF3Z83M04SGX"
title = "Complete file-text IO before publishing successful Results"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for native and interactive text IO

## Decision

I will preserve File.read/write/append signatures and stable error codes while publishing only complete UTF-8, NUL-free text of at most 16 MiB. Native write success requires the complete fwrite, no stream error and successful fclose, including buffered flush.

## Context

Real failed writes could report Ok(4) while producing an empty file, and File.read could report success for bytes later truncated at NUL. Text APIs must not manufacture successful partial values. The three entry points are extracted into a focused runtime module for deterministic cleanup/failure tests.

## Consequences

Preflight rejects invalid/oversized write input before opening a target. Reads validate known length before allocation and probe one extra byte for growth. Every owned stream closes once, preserving an earlier error. IO after opening can still alter files; no atomicity, hard deadline, fsync or binary API is implied. REPL text policy matches while keeping its stricter budget; safe Rust File drop cannot observe late OS close errors, so only explicit unbuffered IO completion is claimed there. [The text IO contract](../docs/FILE_TEXT_IO.md) documents compatibility changes and errors. Native invalid-byte String guards remain tested through direct injection. The unavailable `/decision` skill is replaced by this established format.
