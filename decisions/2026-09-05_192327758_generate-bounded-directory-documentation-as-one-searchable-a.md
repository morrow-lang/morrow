+++
schema_version = 1
id = "01M2XHZ8MEEFVZBZCP6Z4R58S7"
title = "Generate bounded directory documentation as one searchable artifact"
date = "2026-09-05"
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

I will extend parser-based documentation to source directories with deterministic file ordering, module navigation and local text search in a single standalone HTML artifact. Markdown remains available and single-file behavior stays compatible.

## Context

Individual source documentation is implemented, but a library's users need to move between modules and find declarations. A single artifact avoids partial multi-file publication and filename collisions, and local filtering needs no network service or external dependencies.

## Consequences

Recursive discovery has explicit depth, entry, source-byte and file-count limits; symbolic links are not followed. Every source parses before output is installed, and output cannot replace any input inode. Source paths and documentation remain escaped data; the fixed search script only reads text and toggles visibility. Default directories exclude hidden entries and build/dependency directories, with the exclusions documented. Executable doc tests and inferred documentation signatures remain separately gated features. The unavailable `/decision` skill is replaced by the established decision format.
