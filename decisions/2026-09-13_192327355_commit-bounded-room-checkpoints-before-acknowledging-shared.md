+++
schema_version = 1
id = "01M2XHZ87VW2ZMG83T1HE702R3"
title = "Commit bounded room checkpoints before acknowledging shared mutations"
date = "2026-09-13"
status = "accepted"
tags = ["git"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; native and real WebSocket restart tests pass

## Decision

Offer optional local room-state durability through `FERN_WEB_DATA_DIR`. A single writer owns a bounded checkpoint directory, validates actor replies, atomically publishes room state and syncs file/directory storage before the command gateway acknowledges the mutation. Restore tasks and the next identifier into compiled Fern actors. Restart authentication, revisions and command namespaces under fresh resource incarnations.

## Context

Cached browser state is not authoritative durable server state. Replaying uncertain commands across a process restart without retained outcome identity would be unsafe. Room-state checkpoints therefore have a deliberately narrower contract than a durable exactly-once external-effects log. The ephemeral mode remains available when no data directory is configured.

## Consequences

Failed application transitions invalidate their incarnation before recovery. A domain reset retires its actor and restores the last acknowledged checkpoint; a failed recovery leaves the room unavailable. Old commands cannot acquire new meaning after a reset. Corrupt/future checkpoints and concurrent writers fail closed. Directory, lock and temporary-file ownership must remain pinned during publication. If publication succeeded but its final directory sync failed, completion is uncertain and the host must not acknowledge it. Multi-node consensus, replicated durable ownership, backups and external-effect transactions are separate gates.
