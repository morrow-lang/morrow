+++
schema_version = 1
id = "01M2Y644398BT8BSTMJBZMZ3BJ"
title = "Keep OTP supervisor strategies in a pure driver-owned engine"
date = "2026-09-20"
status = "accepted"
tags = ["runtime", "process"]
supersedes = []
superseded_by = []
depends_on = ["01M2XHZ7YXSWSBJYGZJV0V4XG9"]
related_to = ["01M2XHZ7YBPAK2SX7888PMHVWF"]
+++
## Status

Adopted for the isolated engine; language, compiler, ABI and live-actor driver integration remain open.

## Decision

Implement OTP supervisor strategies as a pure, owner-local state machine in `crates/morrow-runtime/src/managed/supervisor/`. The engine owns no processes, reads no clock and performs no scheduling. A driver starts a validated tree, performs the returned actions, and reports acknowledgements, exits, deadlines, retries, whole-second clock readings, parent exits and management requests. Preserve the existing `supervise` API and legacy actor behaviour.

The engine covers startup acknowledgement in declaration order, reverse-order rollback, Permanent/Transient/Temporary restart, one-for-one/one-for-all/rest-for-one, inclusive rolling intensity including zero, failed-restart charging, graceful/infinity/immediate shutdown, parent-exit shutdown, nested branch engines, dynamic child management and significant-child auto-shutdown. Supervisor-induced exits never restart and never count as natural significant completion.

## Context

The typed process model specifies this module boundary and pins OTP 29.0.6 `supervisor.erl`. Monitors and links landed on main without the supervisor engine. An unfinished compiler-and-runtime attempt remains on `task/typed-supervision` and does not compile; this engine is a separate, testable core rather than a merge of that branch.

Driving real actors from inside the engine would pull clocks, heaps and scheduler blocking into a module that must stay deterministic. Independent expected traces against the pinned Elixir supervisor fixtures, plus injected-clock window tests, can verify the policy before any language surface exists.

## Consequences

Morrow programs still cannot start a typed `Supervisor`. Root/actor request adapters, opaque child keys, init acknowledgement builtins, compiler contracts and native registration stay future work; until then the roadmap item for complete supervisor trees remains open. The engine's structural limits match the specification (intensity 0..1024, period 1..86400 s, 1024 children, depth 64, 64 queued requests). Nested branches are separate engine instances the driver composes; the engine does not walk foreign heaps. See `docs/PROCESS_MODEL.md` and `docs/superpowers/specs/2026-09-19-typed-otp-process-model.md`.
