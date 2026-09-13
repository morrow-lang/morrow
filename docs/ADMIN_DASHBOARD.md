# System dashboard

Open `/admin` on the running Fern web server, or follow **System dashboard** in
the application footer. Sign in to the application first; the dashboard reuses
that session automatically. **Refresh snapshot** obtains a new observation;
**JSON status** exposes the same data at `/admin/status` (schema version 1).

The page is rendered by Rust and embedded in the server binary. It requires no
JavaScript, separate monitoring service, additional dependency or frontend build.
Its navigation link is included in the browser asset bundle.

## What it shows

- Server uptime, Fern package version, operating system, architecture, process
  ID and available CPU parallelism.
- Executable size and embedded browser asset bytes; these are file/asset sizes,
  not process memory usage.
- Open WebSocket admission slots, including sockets that have not joined a room,
  and their configured limit.
- Each pinned actor worker’s activity and last observed rooms, namespaces,
  protocol connections and subscriptions.
- Retained authentication sessions, shared ingress use and limits for sessions,
  rooms, namespaces and TCP admission.
- Ephemeral or checkpointed room storage. Filesystem paths are not disclosed.

Workers publish small snapshots outside domain callbacks. Reading them does not
enqueue work behind a busy Fern actor. `Busy` means the owner is handling a request
or expiration pass; it does not indicate CPU utilization. Its counts remain those
of its last completed observation until the work returns. `Stopped` records worker
termination. Authentication expires retained sessions on its normal periodic tick.
Snapshots from different owners and admission counters are independent observations,
not a globally atomic view. Dashboard authentication still uses bounded normal
ingress and can return HTTP 503 under saturation.

The page does not currently sample CPU usage, resident memory, individual actor
heaps, GC pauses or per-room traffic. It has no mutation, restart, arbitrary-code
execution or remote node controls. It is a view of this server, not a cluster
management interface.

## Access and caching

The preview has one shared access key. Every valid preview session can read the
dashboard; there is no separate administrator role. Applications needing separate
operator access must add that policy before sharing their normal application key.
The HTML and JSON routes reject missing, expired or revoked sessions and explicit
foreign origins. Neither endpoint displays access keys, session/CSRF tokens, room
identifiers, task contents or checkpoint paths.

Dashboard responses, including denials, use `Cache-Control: no-store`. The offline
worker only handles its explicit public-asset allowlist, which excludes all admin
routes. CSP disables scripts and framing. Public deployment retains the web
preview’s HTTPS and authentication requirements. Authentication sessions still
restart with the server; this dashboard does not change that contract.

## Acceptance

On 2026-09-13, 12 optimized owner tests passed, including a deliberately blocked
domain callback, full ingress admission, real room/connection transitions and
stopped-state publication after resource cleanup. Nine real HTTP/WebSocket tests
passed, including missing/revoked sessions, foreign origins, no-store responses,
secret exclusion, actual socket occupancy and checkpointed storage reporting.
The offline-cache policy explicitly rejects all admin routes.

The rebuilt macOS ARM64 server passed the full browser application acceptance
suite. Its dashboard was also inspected in the browser on desktop and a
375-pixel phone viewport. Metrics stack below 380 pixels and the worker table
scrolls within its panel. The browser gate uncovered an initial-document race;
a red/green transition regression now requires the requested URL and completed
load within the existing deadline before a new test page is used.

This build is 2,917,424 bytes, with SHA-256
`843a594f6891d72af558d91f1a688bed6d40e9274ac6d785e69904cf115e6a94`
and browser asset revision
`552de9b0a8ca81ba9ec70c71acc15526b66b78baf87aad0a48021368de953c8c`.
Earlier cross-platform artifact measurements describe their own checkpoints.

The final macOS `cargo xtask check` passes formatting, notices, workspace Clippy,
1,933 Rust tests across 254 suites, 305 native fixtures, 19 examples, 63 dynamic
compatibility programs, 295 atomic rejections and 64+192 fuzz cases.
