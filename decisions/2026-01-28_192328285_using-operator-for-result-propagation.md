+++
schema_version = 1
id = "01M2XHZ94XCDXX64P20A36RT9A"
title = "Using ? operator for Result propagation"
date = "2026-01-28"
status = "accepted"
tags = []
supersedes = ["01M2XHZ956F0YGKSX96XVEVE3R"]
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will use the `?` operator (Rust-style, postfix) for Result propagation, keeping `<-` only inside `with` expressions.

## Context

After writing real examples, the postfix `?` works better than prefix `<-` because: (1) you see WHAT might fail before the `?`, not after, (2) it's familiar from Rust which is widely known, (3) it chains naturally `foo()?.bar()?.baz()?`, (4) it integrates cleanly with `let` bindings: `let x = fallible()?`. The `<-` syntax is preserved only inside `with` blocks for complex error handling, similar to Haskell's do-notation where `<-` is scoped.

## Consequences

The lexer needs `?` as TOKEN_QUESTION. The `<-` token (TOKEN_BIND) is only valid inside `with` blocks. Simple error propagation uses `let x = f()?`, complex handling uses `with x <- f(), ...`.
