+++
schema_version = 1
id = "01M2XHZ8WM7BDZ14SZTGXSFGCH"
title = "Erlang-pattern hardening pass: explicit link context, deterministic supervision clock, and strategy child tables"
date = "2026-02-06"
status = "accepted"
tags = ["testing"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will harden actor supervision semantics by (1) requiring explicit current-process context for `spawn_link`, (2) adding `actors.demonitor` plus normal-exit classification (`normal`/`shutdown` do not auto-restart), (3) switching restart-intensity timing to a deterministic runtime clock API, and (4) implementing supervisor child-table-driven `one_for_all`/`rest_for_one` strategies alongside `one_for_one`.

## Context

The baseline supervision implementation was functionally useful but still fragile relative to Erlang/OTP behavior: implicit link-parent selection, no demonitor path, wall-clock-dependent restart windows, and no multi-child strategy semantics. The next reliability step required algorithmic behavior improvements rather than API renaming.

## Consequences

Runtime now exposes `fern_actor_set_current/fern_actor_self`, `fern_actor_clock_set/advance/now`, `fern_actor_demonitor`, `fern_actor_supervise_one_for_all`, and `fern_actor_supervise_rest_for_one`; checker/codegen now type-check/lower the new `actors.*` APIs; and supervisor child tables track child ids/order/strategy to drive restart targeting. Coverage is anchored by `test_runtime_actor_spawn_link_requires_current_actor_contract`, `test_runtime_actor_demonitor_stops_down_notifications_contract`, `test_runtime_actor_supervision_uses_deterministic_clock_contract`, `test_runtime_actor_supervise_one_for_all_restarts_all_children_contract`, and `test_runtime_actor_supervise_rest_for_one_restarts_suffix_contract`.
