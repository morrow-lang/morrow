+++
schema_version = 1
id = "01M2XHZ88X8YAKAQF19WW748BJ"
title = "Preserve bounded inline value-match arms"
date = "2026-09-12"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for executable source compatibility

## Decision

I will accept comma-separated inline value-match arms through the existing patterns, guards, AST and typed lowering, with canonical multiline formatting.

## Context

The unchanged C parser/formatter corpus exposed rejection of executable inline matches. Condition-only matches remain indented and with-handlers retain their existing parsing.

## Consequences

A comma followed by a balanced pattern/guard segment with a top-level arrow belongs to the nearest unclosed inline match; other commas remain with the enclosing expression. Group a match before a following caller lambda, or a nested match before subsequent outer arms. Parsing never retries based on inferred types. Seven bounded inline tests, thirty existing/tab parser and formatter tests, exact native evaluation-order output and 512 unchanged seeded cases per compiler pass. No new ABI is introduced.
