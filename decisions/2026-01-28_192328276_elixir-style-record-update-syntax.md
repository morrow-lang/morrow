+++
schema_version = 1
id = "01M2XHZ94MKG435TM6N1H2DE7E"
title = "Elixir-style record update syntax"
date = "2026-01-28"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will use `%{ record | field: value }` syntax for record updates instead of `{ record | field: value }`.

## Context

The original `{ record | field: value }` syntax conflicts with the "no braces" philosophy - Fern uses indentation, not braces, for control flow. Using `%{...}` for record updates matches map literal syntax `%{"key": value}` and is inspired by Elixir. This creates consistency: both maps and record updates use `%{...}`.

## Consequences

Record update syntax is `%{ user | age: 31 }`. Map literals are `%{"key": value}`. Braces without `%` are not used.
