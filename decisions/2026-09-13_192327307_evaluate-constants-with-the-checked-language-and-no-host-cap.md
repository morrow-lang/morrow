+++
schema_version = 1
id = "01M2XHZ86B1MV3P7KVN75QMHYR"
title = "Evaluate constants with the checked language and no host capabilities"
date = "2026-09-13"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; focused checker, REPL, editor and native tests pass
* **Decision**: Parse `const name[: Type] = comptime:` as a value declaration using the ordinary body grammar. After type specialization and Result proof, run its closed initializer in the bounded Rust evaluator and replace it with ordinary typed constant data. Reject unresolved constant types, including unused polymorphic initializers.
* **Context**: Compile-time computation should use Fern's own arithmetic and collection semantics without spawning a native executable or granting the compiler filesystem/network access. Empty unconstrained constants must not become unevaluated generic templates that hide effects.
* **Consequences**: All constants evaluate under shared work/data limits. Host effects, output, actors and foreign calls reject before execution; failures leave the prior REPL session intact. Public annotations, module visibility, formatting and LSP value presentation follow existing conventions. Embedded aggregates may allocate their representation at runtime, but do not rerun their initializer computation. AST reflection, general inline comptime expressions and opaque host resources are outside this constant-data feature. See `docs/COMPTIME.md`.
