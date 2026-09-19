+++
schema_version = 1
id = "01M2XHZ895KGFY1N3QBGV9AGWQ"
title = "Preserve consistent tab indentation during compiler migration"
date = "2026-09-12"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = ["01M2XHZ97B4V992VGPE1PRM59K"]
related_to = []
+++
## Status

Accepted for executable source compatibility

## Decision

I will accept consistently tab-indented Fern source using eight-column tab stops, preserve byte spans, and keep canonical formatting at four spaces per layout level.

## Context

The legacy seeded parser/formatter corpus begins with a valid tab-indented program. Rejecting every tab in Rust broke that executable C-source contract. [Decision 3](2026-01-26_192328363_python-style-indentation-syntax.md) rejects mixed tabs/spaces rather than consistently tab-indented source.

## Consequences

Significant code indentation must use one style per source and cannot mix tabs and spaces in a prefix. Blank/comment-only lines and ordinary delimiter continuation whitespace do not select the style; embedded suites do. Horizontal tabs between tokens remain whitespace. Source/token/layout bounds and string/comment contents remain intact. Both default Rust and explicit C reference run the original fuzz smoke corpus; the separate Rust mutation corpus remains required.
