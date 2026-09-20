+++
schema_version = 1
id = "01M2Y64439F6Z441PY2FET65SP"
title = "Carry live viewer counts on room snapshots and publish the demo on one Fly machine"
date = "2026-09-20"
status = "accepted"
tags = ["web", "protocol"]
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2XHZ83PY052N3N96FTQQ48R", "01M2XHZ882DG9GHZBAYY93GFAR", "01M2XHZ87M861ZS6BSP8P6753S", "01M2XHZ843GP176BHPGQH5CYR0", "01M2Y9Z4A32Z1E805G8YQJGJKN"]
+++
## Status

Adopted for the checklist demo; enabling the Fly.io and documentation workflows is owner setup, not part of this record's code change.

## Decision

Carry each room's live viewer count on the existing snapshot as `viewers`, the number of physical WebSocket connections currently attached to that room's owner. Republish the snapshot when a connection joins, leaves, expires or is revoked, without advancing the room revision. Browsers accept a same-revision presence snapshot and keep pending commands. The
compiled Morrow function `presence_text` produces `N viewing` while online and
blank while offline, because a cached count is stale. The host writes that string
onto the existing header element; putting the count in the view list would make a
full 100-task room exceed the WebAssembly 511-field aggregate bound.

Publish the existing checklist demo as a single Fly.io Machine from a scratch image of the static x86-64 server, and publish the generated documentation site to `morrow-lang/morrow-lang.github.io`. Keep both GitHub Actions workflows disabled until the owner sets the documented variables and secrets. Do not run visitor-submitted Morrow on this site.

## Context

Connections were already counted per room for the dashboard. A second presence channel would have duplicated delivery, revision and cluster-forwarding rules. Putting the count on the snapshot reuses the owner's existing publication path and the cluster's snapshot forwarding.

A public playground that compiles visitor code is a different product. The preview server embeds one fixed application and has no sandbox for native Morrow, which can reach files, processes, HTTP and FFI. A browser-side WebAssembly playground would cover only the portable subset; a per-run Fly Machine would cover native execution but needs abuse controls. Neither is this decision.

One process is required because room owners and the viewer count are local to that process. Multi-node deployment remains the separate cluster path. x86-64 static servers have been built and exercised in CI; a Fly deploy is the first sustained public x86-64 run.

## Consequences

Presence never participates in command ordering or conflict detection. Counts are not persisted and start over on every process start. Duplicate tabs count separately; a resumed namespace replaces its previous connection rather than adding one. Anyone with the shared demo access key can edit the room within server bounds. Do not scale the Fly app above one machine. Custom domains, accounts and a playground remain follow-up work. See `docs/DEPLOY.md`, `docs/WEB_PREVIEW.md` and `protocol/README.md`.
