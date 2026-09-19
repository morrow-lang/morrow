+++
schema_version = 1
id = "01M2XHZ84RTFXMPSHMXXVC1WWX"
title = "Run source actors interactively under deterministic virtual time"
date = "2026-09-13"
status = "accepted"
tags = ["testing", "runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; REPL and FernSim transcript/replay tests pass
* **Decision**: Reuse validated actor continuation IR in the Rust interpreter, with a session-owned FIFO scheduler, selective mailboxes, monotonically issued Pids and virtual deadlines. Retain dormant actors and their original checked code across interactive entries. Expose read-only reports, explicit cancellation and a bounded source-transcript FernSim bridge.
* **Context**: Syntax accepted by the compiler should be useful for interactive development and reproducible failure tests. Reimplementing a separate source actor model would drift from native continuation semantics; real sleeps would make short simulations slow and timing-dependent.
* **Consequences**: The virtual clock advances between runnable turns and jumps to the next deadline when idle. New bindings roll back after an entry failure, while already executed actor effects retain their meaning. Restarted actors have new identities; stale Pids stay stale. Limits cover actors, messages, value/code graphs, transcript bytes and evaluator work. Explicit `:stop`, reset, quit and EOF run pending cleanup. Rust embedders explicitly stop a session when source cleanup is required. Three-seed message models and an independent eight-seed supervision/fault/cancellation campaign compare exact output and scheduler state, then replay the reports. See `docs/REPL_ACTORS.md`.
