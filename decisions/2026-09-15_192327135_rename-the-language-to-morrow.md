+++
schema_version = 1
id = "01M2XHZ80Z00DW0VYT039EPC6H"
title = "Rename the language to Morrow"
date = "2026-09-15"
status = "accepted"
tags = ["naming"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; migration verification is tracked in ROADMAP.md

## Decision

Rename Fern to **Morrow**, using “the Morrow programming language” in public introductions. The command is `morrow`, source files use `.mr`, the repository moves to `morrow-lang/morrow`, and the canonical domain is `morrow-lang.org`; `morrow-lang.dev` will redirect permanently to it. Rename all workspace `fern*` packages and component directories to `morrow*`, including the runtime and JSON packages. Rename the contributor style guide to `MORROW_STYLE.md`. Use a text mark until a new logo is available.

## Context

Fern collides with fern-lang.org, fern-lang.com, buildwithfern.com and a Handmade Network language. Niklas has selected Morrow following his naming research: no existing trademark covering programming tools was identified in that research; Daan Leijen's 2004 Morrow is an inactive academic research language, and the existing `morrow` crate on crates.io is an unrelated Minecraft mod SDK. This records the supplied research and decision, not a new legal clearance. The EUIPO/DPMA check for classes 9/42 remains Niklas's open follow-up. Both domains were unregistered on 2026-09-15, and registration and the `.dev` redirect remain his responsibility.

## Consequences

Workspace package `morrow` and the `morrow` executable retain the selected development names. Because crates.io's `morrow` name is occupied, that workspace package is not publishable under its current name; future registry distribution must use `morrow-lang` for the compiler package and `morrow-*` for components. No crates are published by this migration. Old source extensions, editor identifiers and component names are replaced together; native ABI artifacts must be rebuilt with the renamed compiler/runtime. Dated reports, benchmark results and earlier decision entries retain their original names, dates and measurement context with an explanatory rename note. The migration is a staged, conventionally committed series with `cargo xtask check` between stages.

## Data compatibility

Existing cluster hash-domain bytes and checkpoint placement headers retain their legacy spelling. These are persisted identity inputs, not public branding: changing them would move room ownership or reject acknowledged checkpoints during a language rename. Their independent fixed-vector tests remain in place.
Historical note: Fern was renamed to Morrow on 2026-09-15; earlier decision entries below retain their original wording.
