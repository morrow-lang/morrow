+++
schema_version = 1
id = "01M2XHZ8X5HGGCKBX1ZBN96AGV"
title = "Erlang-inspired actor monitoring baseline: `DOWN(...)` notifications + explicit restart primitive"
date = "2026-02-06"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will model monitor behavior after Erlang by adding one-way monitor registrations that emit `DOWN(pid, reason)` messages on exit, while keeping linked-exit `Exit(pid, reason)` delivery and adding an explicit `actors.restart(pid)` primitive for baseline supervision workflows.

## Context

The prior supervision slice introduced `spawn_link` and linked exit notifications, but did not cover monitor semantics or restart APIs. We needed a practical, test-first step that reflects Erlang process semantics closely enough for adoption while staying within current runtime constraints (mailbox/scheduler baseline, no full supervisor tree policies yet).

## Consequences

Checker/codegen/runtime now expose `actors.monitor(Int, Int) -> Result(Int, Int)` and `actors.restart(Int) -> Result(Int, Int)`, runtime stores monitor registrations per actor, `fern_actor_exit` emits `DOWN(...)` to monitors and `Exit(...)` to linked parents, and restart returns a new actor id preserving name/link baseline and monitor registrations. Coverage is anchored by `test_check_actors_monitor_returns_result`, `test_check_actors_restart_returns_result`, `test_codegen_actors_monitor_calls_runtime`, `test_codegen_actors_restart_calls_runtime`, and `test_runtime_actor_monitor_and_restart_contract`.
