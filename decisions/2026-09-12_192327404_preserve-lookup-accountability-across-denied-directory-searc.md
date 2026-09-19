+++
schema_version = 1
id = "01M2XHZ89CRBCE1J0FQMVJY117"
title = "Preserve lookup accountability across denied directory search"
date = "2026-09-12"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted for the native bootstrap cache

## Decision

I will record an explicit inaccessible-subtree inventory marker only when directory listing and effective-user search both fail with `EACCES`, and create cold-worker artifacts under a private `umask 077`.

## Context

Linux's ordinary `umask 002` made generated metadata group-writable and therefore correctly rejected by the private-cache validator. After isolating the worker mask, recursive library inventories reached `/usr/lib/ssl/private`, whose root-owned target denied search. Rejecting that unrelated subtree prevented the quality checker from starting; silently ignoring unreadable directories would hide headers that a compiler can still open by known name.

## Consequences

Use `faccessat` with `AT_EACCESS` on Linux and macOS. Re-evaluate the marker on every cache lookup so gained access changes the inventory. Unreadable-but-searchable directories and other access errors still reject; fixed traversal bounds and cycle checks remain. Worker-only permissions do not alter the final checker's caller mask. Red-first permission-transition tests pass with private debug, release and sanitizer helpers on both platforms; cold-cache and caller-mask regressions cover `002` and `000`/`002`/`027` respectively.
