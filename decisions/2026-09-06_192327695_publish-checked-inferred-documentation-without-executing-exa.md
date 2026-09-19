+++
schema_version = 1
id = "01M2XHZ8JFFHWY3JFJS82FTJ0R"
title = "Publish checked inferred documentation without executing examples"
date = "2026-09-06"
status = "accepted"
tags = ["docs"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for Rust migration completion

## Decision

I will add explicit `doc --inferred` generation using one complete library check and bounded reusable source schemes per module graph. Original headers and documentation remain, supplemented by resolved signatures and intrinsic requirements.

## Context

Source-only documentation cannot explain omitted private types. Rechecking the program per declaration scales poorly and risks inconsistent generic identities; backend specializations erase source patterns and do not describe reusable functions. A checked library pipeline now validates all bodies without inventing main.

## Consequences

Default documentation remains parser-only. Checked mode resolves current imports, preserves exact source anchors, rejects invalid graphs and enforces aggregate graph/metadata/output budgets. Source contents are borrowed from bounded project caches, with actual loaded and cached copies charged once. Every documented source and loaded dependency is protected against output replacement, including hardlinks. Generated names never appear as user types, and directory search/escaping remain shared. The unavailable `/decision` skill is replaced by the established decision format.
