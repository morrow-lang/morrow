+++
schema_version = 1
id = "01M2XHZ8WWTAG05FJ035REKBWM"
title = "Erlang-style process lifecycle baseline: exited PIDs become dead and non-routable"
date = "2026-02-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Accepted
* **Decision**: I will enforce Erlang-inspired process lifecycle semantics where `fern_actor_exit` transitions a process to a dead PID state, dead actors reject send/receive/mailbox/scheduler participation, and manual `fern_actor_restart` is allowed only from dead actors.
* **Context**: Supervision intensity and monitor/restart contracts were in place, but exited actors remained routable in runtime state, which violated core process semantics and made supervision behavior less reliable. We needed a concrete lifecycle boundary so supervision algorithms operate on real process death, not soft notifications.
* **Consequences**: Runtime actor records now track alive/dead state. `fern_actor_send`, `fern_actor_receive`, `fern_actor_mailbox_len`, scheduler selection, `fern_actor_monitor`, and `fern_actor_supervise` all require live actors. `fern_actor_exit` marks actors dead before notifications/restart handling, and `fern_actor_restart` now requires a dead source actor id. Coverage is anchored by `test_runtime_actor_exit_marks_actor_dead_contract`.
