+++
schema_version = 1
id = "01M2XHZ8Y6FDJM5C7MR9Q2DP25"
title = "Gate C actor runtime core baseline: FIFO mailbox + round-robin scheduler tickets"
date = "2026-02-06"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Accepted
* **Decision**: I will implement Gate C actor runtime core as an in-memory runtime with per-actor FIFO mailboxes and a round-robin scheduler queue driven by send-time scheduler tickets, exposed through `fern_actor_spawn/send/receive/mailbox_len/scheduler_next` with `start/post/next` compatibility aliases.
* **Context**: Gate C Task 2 required concrete runtime behavior for `spawn`, `send`, `receive`, and scheduler operations, not placeholder acknowledgments. Existing actor runtime only returned monotonic ids, dropped posted messages, and returned `Err(FERN_ERR_IO)` for reads. We needed deterministic semantics that can be regression-tested immediately and used by both Fern stdlib calls and runtime C-ABI tests.
* **Consequences**: Runtime actor APIs now provide mailbox FIFO delivery and deterministic scheduler ordering (`a, b, a` shape for the covered scenario) while preserving compatibility for `actors.start/post/next`. Coverage is anchored in `tests/test_runtime_surface.c` via `test_runtime_actors_post_and_next_mailbox_contract` and `test_runtime_actor_scheduler_round_robin_contract`, and compatibility docs are updated to treat this as Gate C baseline behavior.
