+++
schema_version = 1
id = "01M2XHZ8QW87TF4271ZVTZV96P"
title = "Define entry errors and guard legacy runtime preconditions"
date = "2026-09-05"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for Rust migration completion

## Decision

I will accept `main -> Result((), E)` for every concrete error type, exit zero for Ok and one for Err after deferred cleanup, and report `fern: main returned Err` for an unhandled entry error. Runtime faults take precedence. Existing direct-valued list access keeps its source signature and reports invalid access through the explicit Rust fault context.

## Context

DESIGN specifies Result entry points but does not define a universal Error type or an error-display protocol. The existing List.get/head signatures have incompatible direct-value and recoverable descriptions. Their native assertions are not a safe execution contract. String repetition can overflow its allocation size from a tiny input.

## Consequences

Rust-generated List.get/head failures run cleanup and never load out-of-bounds storage. Shared C helpers independently report the same failures before access in debug and release builds; their legacy callers do not receive the Rust cleanup protocol. General error rendering and recoverable indexing APIs remain separate work. String.repeat permits at most 16,777,216 content bytes, checks before multiplication/allocation, and returns empty immediately for empty input or nonpositive counts. Rust checks before calling C so cleanup executes; the legacy C ABI independently rejects oversized requests with the same diagnostic and exit 1, without Rust's cleanup protocol. The REPL applies this language limit before its stricter interactive storage limit. No arbitrary error payload is printed as an address, and no failure is replaced with an empty string. The unavailable `/decision` skill is replaced by the established decision format.
