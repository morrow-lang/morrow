# System dashboard

> Fern was renamed to Morrow on 2026-09-15; historical measurements and acceptance records below retain their original names, paths and results.

Open `/admin` on a loopback Morrow web server, or follow **System dashboard** in
the application footer. Public origins omit the dashboard: the footer link is not
shown and `/admin` is not served. `MORROW_WEB_ADMIN=1` re-enables it; `=0` hides
it on loopback. Open the application first so it can issue a session;
the dashboard reuses that session automatically. **Refresh snapshot** obtains a new observation;
**JSON status** exposes the same data at `/admin/status` (schema version 1).

The page is rendered by Rust and embedded in the server binary. It requires no
JavaScript, separate monitoring service, additional dependency or frontend build.
The footer link is omitted when the dashboard is not served.

## What it shows

- Server uptime, Morrow package version, operating system, architecture, process
  ID and available CPU parallelism.
- Executable size and embedded browser asset bytes; these are file/asset sizes,
  not process memory usage.
- Current resident process memory (RSS) and peak resident memory, displayed in MiB
  and exposed as byte counts in JSON. These include the whole server process,
  not just Morrow-managed heaps; browser memory belongs to a separate process.
- Open WebSocket admission slots, including sockets that have not joined a room,
  and their configured limit.
- Each pinned actor worker’s activity and last observed rooms, namespaces,
  protocol connections and subscriptions.
- Retained authentication sessions, shared ingress use and limits for sessions,
  rooms, namespaces and TCP admission.
- Ephemeral or checkpointed room storage. Filesystem paths are not disclosed.

Workers publish small snapshots outside domain callbacks. Reading them does not
enqueue work behind a busy Morrow actor. `Busy` means the owner is handling a request
or expiration pass; it does not indicate CPU utilization. Its counts remain those
of its last completed observation until the work returns. `Stopped` records worker
termination. Authentication expires retained sessions on its normal periodic tick.
Snapshots from different owners and admission counters are independent observations,
not a globally atomic view. Dashboard authentication still uses bounded normal
ingress and can return HTTP 503 under saturation.

The page does not currently sample CPU usage, individual actor
heaps, GC pauses or per-room traffic. It has no mutation, restart, arbitrary-code
execution or remote node controls. It is a view of this server, not a cluster
management interface.

Memory is sampled directly from the operating system on each snapshot, without a
subprocess or new dependency. macOS supplies resident bytes through its process
task information API; Linux supplies an approximate resident-page count through
bounded `/proc/self/statm` reads. Peak RSS comes from process resource accounting,
with each platform’s units normalized to bytes. Readings are independent OS
observations. A failed or unsupported reading is `null` in JSON and **Unavailable**
on the page, never a fabricated zero.

## Access and caching

The preview has no shared access key. Every valid preview session can read the
dashboard when it is enabled; there is no separate administrator role. Public
deployments hide the page. Applications needing operator access on a public
origin must set `MORROW_WEB_ADMIN=1` and add their own policy.

The HTML and JSON routes reject missing, expired or revoked sessions and explicit
foreign origins. Neither endpoint displays session/CSRF tokens, room
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

The original dashboard build at `f76fd57` is 2,917,424 bytes, with SHA-256
`843a594f6891d72af558d91f1a688bed6d40e9274ac6d785e69904cf115e6a94`
and browser asset revision
`552de9b0a8ca81ba9ec70c71acc15526b66b78baf87aad0a48021368de953c8c`.
Earlier cross-platform artifact measurements describe their own checkpoints.

That checkpoint’s macOS `cargo xtask check` passes formatting, notices, workspace Clippy,
1,933 Rust tests across 254 suites, 305 native fixtures, 19 examples, 63 dynamic
compatibility programs, 295 atomic rejections and 64+192 fuzz cases.

The subsequent memory addition passed three sampler tests on native macOS and
Linux, plus Clippy on both platforms and the dashboard HTTP regressions. An
isolated process touches a 32 MiB allocation to verify that measured RSS rises;
the Linux run observed 4,411,392 → 37,969,920 resident bytes. Parser and unit tests
independently check malformed data, overflow and platform-specific peak units.
The rebuilt macOS ARM64 demo reports both memory fields in its authenticated
dashboard and preserves its checkpointed tasks. Its server is 2,917,536 bytes,
with SHA-256
`b1f11c1dab1a5afdc566682c58bfedbd8c96b79983633193bdc04fbe6c19df7b`;
the browser asset revision is unchanged.

The memory addition’s complete macOS `cargo xtask check` passes formatting,
notices, workspace Clippy, 1,936 Rust tests across 254 suites, 305 native fixtures,
19 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192
fuzz cases.
