+++
schema_version = 1
id = "01M2XHZ956F0YGKSX96XVEVE3R"
title = "Using <- operator instead of ?"
date = "2026-01-27"
status = "superseded"
tags = []
supersedes = []
superseded_by = ["01M2XHZ94XCDXX64P20A36RT9A"]
depends_on = []
related_to = []
+++
## Decision

I will use the `<-` operator for Result binding instead of the `?` operator.

## Context

Initially considered Rust's `?` operator (postfix), but this has clarity issues: (1) it comes at the END of the expression, so you don't immediately see that an operation can fail, (2) `?` is overloaded in many languages (ternary, optional, etc.), making it less obvious. The `<-` operator (from Gleam/Roc) addresses both issues: it comes FIRST so failure is immediately visible, it reads naturally as "content comes from read_file", and it's not overloaded with other meanings.

## Consequences

All error handling examples use `<-` syntax. The lexer must recognize `<-` as a distinct token. Error messages reference `<-` in explanations.
