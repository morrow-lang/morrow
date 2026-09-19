+++
schema_version = 1
id = "01M2XHZ8CYNPJDCKCDTDVCW9A1"
title = "Retain native test identity until descendant cleanup is complete"
date = "2026-09-06"
status = "accepted"
tags = ["testing"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for the native helper and safe Rust adapter

## Decision

I will supervise native unit/doc tests with a separate trusted native component that retains the child with WNOWAIT through group cleanup, then reaps exactly that child. Safe Rust receives a bounded versioned binary frame and never signals numerical test process groups or creates detached stream-reader threads.

## Context

The old adapter observed/reaped a child with try_wait before invoking external group kill; ownership of that numerical group could already have ended. Escaped pipe holders could also strand detached readers. Source-validated failing lifecycle/protocol tests preceded the replacement. The unavailable `/decision` skill is replaced by this established format.

## Consequences

The helper owns private fixed spools, held file identities, bounded nonblocking capture/publication and cleanup. Rust retains the taken stdin liveness guard through wait, validates canonical fields, complete lengths/trailer/EOF and helper status, and removes only its empty private parent. Each stream is capped at256KiB; positive test deadlines up to60s initiate cleanup, followed by a1s publication allowance. Kernel waits and escaped groups have explicit limits. Native exit125 remains a test status, separate from transport failure. Debug/Rust-release builds include fern-test-supervisor; missing/incompatible helpers fail with no fallback. The standalone OS ABI uses fixed bounded storage without Fern GC or authored dynamic allocation. Existing language Result/test APIs remain unchanged.
