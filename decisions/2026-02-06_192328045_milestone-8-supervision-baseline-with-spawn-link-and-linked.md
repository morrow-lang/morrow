+++
schema_version = 1
id = "01M2XHZ8XDC6HFVRP6RFVM1Q82"
title = "Milestone 8 supervision baseline with `spawn_link` and linked `Exit(...)` notification"
date = "2026-02-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Accepted
* **Decision**: I will add a first supervision contract now by supporting `spawn_link(fn)` in checker/codegen and runtime linked-exit delivery via `fern_actor_exit`, with deterministic baseline linking to the most recently spawned actor id.
* **Context**: Gate D passed and roadmap focus shifted to milestone polish, but supervision remained an unstarted milestone gap even though the language design documents `spawn_link`/`Exit(...)` behavior. We needed a minimal, testable slice that introduces supervision semantics without waiting for full actor-process execution and restart machinery.
* **Consequences**: `spawn_link(...)` now type-checks and lowers to `fern_actor_spawn_link`, runtime actor records track a linked parent id, and `fern_actor_exit(actor_id, reason)` enqueues `Exit(actor_id, reason)` messages to the linked supervisor mailbox. Coverage is anchored by `test_check_spawn_link_returns_int`, `test_codegen_spawn_link_calls_runtime`, and `test_runtime_actor_spawn_link_exit_notification_contract`. Full monitor/restart policies remain future milestone work.
