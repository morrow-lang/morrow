+++
schema_version = 1
id = "01M2XHZ930VVHD50XZN9FQTR3Y"
title = "HexDocs-style documentation generation"
date = "2026-01-28"
status = "accepted"
tags = ["docs"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will implement automatic documentation generation with two systems: (1) a built-in `fern doc` command for Fern code that generates HTML from `@doc` comments, and (2) a custom doc generator for the C compiler source code.

## Context

Good documentation is essential for adoption. Considered several approaches: (1) Doxygen for C code - industry standard but dated look, (2) Sphinx + Breathe - modern but complex setup, (3) Custom solution - tailored to our needs. For Fern language docs, a built-in command like `cargo doc` or `mix docs` provides the best developer experience. For compiler docs, a custom solution lets us match FERN_STYLE conventions and maintain a consistent look across both documentation systems.

## Consequences

Need to implement `fern doc` command that parses `@doc` comments and generates searchable HTML. Need to build a C doc extractor that understands our comment conventions. Both should share HTML templates for consistent styling. Documentation generation will be added to CI to keep docs up-to-date.
