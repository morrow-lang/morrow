+++
schema_version = 1
id = "01M2ZC8W0GA20C180R6AXHHNMJ"
title = "Suspend the Fly demo Machine when idle"
date = "2026-09-20"
status = "accepted"
tags = ["web"]
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2Y64439F6Z441PY2FET65SP"]
+++
## Decision

Suspend the public Fly demo Machine when it is idle. `auto_stop_machines = "suspend"`, `auto_start_machines = true` and `min_machines_running = 0`.

## Context

The demo previously kept one Machine running at all times so live viewer counts and WebSocket rooms would survive quiet periods. That billed CPU and RAM continuously on a `shared-cpu-1x` / 256 MB Machine. The user asked to suspend it to save money.

`min_machines_running` must be `0`; with `1` the single Machine would never idle. `suspend` is preferred to `stop` because a resume is typically a few hundred milliseconds and the 256 MB Machine is within Fly's suspend limit. A deploy or host migration still discards the snapshot and cold-starts. The attached volume is billed either way and is not required for the snapshot.

## Consequences

The first request after idle waits for resume or a cold start. Volume checkpoints survive. In-memory sessions, sockets and viewer counts do not survive a discarded snapshot. Health checks apply only while the Machine is running. See `docs/DEPLOY.md`.
