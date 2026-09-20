+++
schema_version = 1
id = "01M2ZAR3SQ5AFRQEW9KNSFTBDW"
title = "Serve the system dashboard on public origins"
date = "2026-09-20"
status = "accepted"
tags = ["web"]
supersedes = ["01M2YB3X69VEV1YE93DWP8WGSF"]
superseded_by = []
depends_on = []
related_to = ["01M2Y9Z4A32Z1E805G8YQJGJKN"]
+++
## Decision

Serve the system dashboard on public origins by default. `MORROW_WEB_ADMIN=0` still hides `/admin` and the footer link.

## Context

Decision175 hid `/admin` on non-loopback origins after automatic sessions made the page reachable to anyone who loaded the demo. A review of the snapshot shows it is read-only process metrics: version, OS, architecture, process ID, uptime, RSS, worker occupancy, admission counters and, when clustered, local node identity and stream counts. It does not include room contents, credentials, session tokens, CSRF nonces or filesystem paths, and it cannot mutate the server.

Those fields are reconnaissance, not secrets. The public demo is already an open whiteboard; occupancy and limits are also documented. The user asked to show the page again if that review held.

## Consequences

The Fly demo shows **System dashboard** after a same-origin session. Operators who do not want the page set `MORROW_WEB_ADMIN=0`. Do not add mutation, room contents or credentials to the snapshot without a new access policy. See `docs/ADMIN_DASHBOARD.md`.
