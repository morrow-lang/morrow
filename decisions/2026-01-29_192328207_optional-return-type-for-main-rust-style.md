+++
schema_version = 1
id = "01M2XHZ92FKA4TSPEXMRZD1P59"
title = "Optional return type for main() (Rust-style)"
date = "2026-01-29"
status = "accepted"
tags = ["rust"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will allow omitting the return type for `main()` only, defaulting to Unit with automatic `ret 0`.

## Context

Writing `fn main() -> Int: 0` for simple programs that don't need a return value is tedious. Rust allows both `fn main()` (Unit return) and `fn main() -> Result<(), E>` (explicit return). We adopt a similar approach: `fn main():` defaults to Unit return and auto-returns 0 (success exit code), while `fn main() -> Int:` requires an explicit integer return. This special case applies ONLY to main() - other functions still require explicit return types or use type inference. This provides ergonomic shorthand for scripts and simple programs while maintaining explicitness for library code.

## Consequences

The type checker treats `main()` with no return type as returning Unit. The code generator emits `ret 0` for main() with Unit return. Both `fn main():` and `fn main() -> Int:` are valid. Other functions are unaffected.
