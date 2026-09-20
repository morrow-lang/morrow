+++
schema_version = 1
id = "01M2Y9Z4A32Z1E805G8YQJGJKN"
title = "Issue same-origin sessions from GET /session and create missing doc-site parents"
date = "2026-09-20"
status = "accepted"
tags = ["web", "docs"]
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2Y64439F6Z441PY2FET65SP", "01M2YB3X69VEV1YE93DWP8WGSF"]
+++
## Decision

Issue a same-origin session from `GET /session` so the browser connects without a shared access key, the way a Phoenix LiveView page opens a socket after loading. Missing or expired cookies mint a new HttpOnly, SameSite=Strict cookie and CSRF nonce. Origin, CSRF, session limits and WebSocket checks stay in place. `MORROW_WEB_ACCESS_KEY` is optional: when unset, `POST /session` is rejected; when set to 16–256 bytes, keyed login remains available for tests.

`morrow doc --site` creates missing parent directories before canonicalizing the destination, so `cargo xtask docs dist/docs` publishes when `dist/` does not already exist. Destination protection is unchanged: inputs, ancestors, symbolic links and unrelated directories are still refused. Directory `--extras` omit `README.md` so a vrdx collection and `docs/` can both use that filename; pass `docs/README.md` as its own extra to keep the catalog page.

## Context

The preview asked every visitor to type a server-wide secret before the WebSocket would open. Elixir/LiveView does not: the page load establishes the session, then the socket connects. The shared key was preview admission, not per-user authorization, and it blocked the public Fly demo from being used as a normal web app.

Creating a session on GET is unusual compared with POST-only login. The alternative of keeping the key for `/admin` only was rejected for this preview because the complaint was specifically the browser connecting to the server; a separate operator role remains application work. Minting on a revoked cookie as well as a missing cookie lets an expired tab recover without clearing storage; a malformed cookie is still rejected.

The documentation workflow failed because `destination()` canonicalized the parent of `dist/docs` before `dist/` existed. Creating the missing parent, then canonicalizing, matches how operators invoke `cargo xtask docs dist/docs`.

## Consequences

Anyone who can load the origin can edit the shared room. Public origins omit `/admin`; loopback servers still show it after the application opens a session. CSRF, exact Origin, cookie flags and admission bounds remain the client-side controls. Do not present a login form in the checklist UI. See `docs/WEB_PREVIEW.md` and `docs/DEPLOY.md`.
