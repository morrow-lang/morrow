+++
schema_version = 1
id = "01M2XHZ86SQ5AH0VKCF9MG4PW5"
title = "Observe pinned workers without queuing behind their callbacks"
date = "2026-09-13"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; acceptance tracked in the roadmap
* **Decision**: Provide a Rust-rendered, read-only `/admin` dashboard and versioned `/admin/status` JSON response using the preview’s existing session authentication. Publish bounded per-owner snapshots outside native callbacks and read them independently of the worker request queues. Display last-observed counts alongside worker activity and configured admission limits.
* **Context**: The full-stack demo needs inspectable system behavior. Requesting diagnostics through a busy room worker would hide precisely the condition an operator needs to see. A shared preview key does not establish separate administrative roles.
* **Consequences**: Snapshots expose no application contents, identifiers, credentials or filesystem paths. HTML/JSON responses prohibit caching; the offline asset allowlist excludes these routes. Observations across owners are not globally atomic, busy-worker counts can lag, and authentication remains subject to normal ingress admission. A safe native host API also samples OS process resident and peak memory in bytes; platform read failures remain explicit. This adds no server controls, CPU utilization sampling, actor heap introspection or cluster management. The implementation is embedded Rust HTML/CSS with manual refresh and no new dependencies.
