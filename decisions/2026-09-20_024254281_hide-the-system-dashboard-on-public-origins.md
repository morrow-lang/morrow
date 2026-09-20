+++
schema_version = 1
id = "01M2YB3X69VEV1YE93DWP8WGSF"
title = "Hide the system dashboard on public origins"
date = "2026-09-20"
status = "accepted"
tags = ["web"]
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2Y9Z4A32Z1E805G8YQJGJKN"]
+++
## Decision

Serve the system dashboard only on loopback browser origins. Public origins omit the `/admin` routes and the footer **System dashboard** link. `MORROW_WEB_ADMIN=0` or `1` overrides the default.

## Context

Automatic same-origin sessions (Decision174) mean anyone who can load a public origin can obtain a cookie. The dashboard reused that session, so the Fly demo exposed process memory, worker occupancy and admission counters. The user chose not to display the admin page in production, the way a Phoenix app does not ship LiveDashboard to visitors.

Keeping `/admin` behind a shared secret was rejected earlier because the product complaint was the browser connecting, not operator observability. Hiding the page on non-loopback origins matches how local `./dist/morrow-web` and the public demo already differ: loopback origin is implied; Fly sets `MORROW_WEB_ORIGIN` to the public HTTPS origin.

## Consequences

Local loopback servers still show the dashboard after the application opens a session. The public demo does not. Operators who need the page on a public bind set `MORROW_WEB_ADMIN=1` and still need their own access policy. Cached service-worker copies of an older index may keep a dead link until the worker revises. See `docs/ADMIN_DASHBOARD.md`.
