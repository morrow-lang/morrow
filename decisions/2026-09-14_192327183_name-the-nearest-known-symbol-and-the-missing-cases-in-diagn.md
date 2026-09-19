+++
schema_version = 1
id = "01M2XHZ82F3RJQFPJDPECD3WQ9"
title = "Name the nearest known symbol and the missing cases in diagnostics"
date = "2026-09-14"
status = "accepted"
tags = ["naming"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; independent checker, parser and suggestion-distance tests pass

## Decision

Attach a bounded "did you mean" hint to unknown name, function, constructor, record-field, type and module-member diagnostics using optimal string alignment distance over the visible candidates plus a small synonym table. Report the uncovered constructors or scalar cases in non-exhaustive match errors. Replace parser "unsupported in the Rust prototype" wording with the token actually found.

## Context

Misspelled or guessed API names produced the misleading "is private, not exported, or not imported" message, and exhaustiveness errors did not state what was missing. The prototype wording no longer described the shipping compiler.

## Consequences

Suggestions are limited to a distance proportional to the name length, a fixed number of candidates and a fixed name length, so diagnostics stay deterministic and cheap. The synonym table covers common names from other languages (`length`, `upper`, `nth`, `to_int`); it does not attempt cross-module suggestions. Missing-case lists are truncated after a fixed count and are informational; the exhaustiveness proof itself is unchanged.
