+++
schema_version = 1
id = "01M2XHZ96GM600YWJY4YXNBD7Y"
title = "with expression for complex error handling"
date = "2026-01-27"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will provide a `with` expression for complex error handling scenarios where different error types need different responses.

## Context

While `?` handles simple error propagation, sometimes you need to handle different errors differently (e.g., return 404 for NotFound, 403 for PermissionDenied, 401 for AuthError). The `with` expression allows binding multiple Results using `<-` and pattern matching on different error types in an `else` clause, similar to Haskell's do-notation.

## Consequences

The parser must support `with`/`do`/`else` syntax. The `<-` operator is only valid inside `with` blocks. The type checker must verify all error types are handled in the else clause.
