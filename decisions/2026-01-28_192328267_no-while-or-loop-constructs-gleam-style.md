+++
schema_version = 1
id = "01M2XHZ94B2EN7CZ76E3M269F3"
title = "No while or loop constructs (Gleam-style)"
date = "2026-01-28"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will not include `while` or `loop` constructs. Stateful iteration uses recursion with tail-call optimization.

## Context

`while` and `loop` require mutable state between iterations, which conflicts with immutability. The semantics of rebinding inside loops are unclear and error-prone. Gleam takes the same approach: no loops, use recursion. Tail-call optimization makes recursion efficient. `for` loops over collections are kept since they don't require mutation - they're just iteration. Functional combinators (`fold`, `map`, `filter`, `find`) handle most cases elegantly.

## Consequences

Remove `while` and `loop` from grammar. Keep `for` for collection iteration. Recursion is the primary mechanism for stateful iteration. The lexer doesn't need TOKEN_WHILE or TOKEN_LOOP. Simplifies the language considerably.
