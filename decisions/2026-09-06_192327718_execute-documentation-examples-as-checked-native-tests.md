+++
schema_version = 1
id = "01M2XHZ8K62YM2D4XXKRE8WQFY"
title = "Execute documentation examples as checked native tests"
date = "2026-09-06"
status = "accepted"
tags = ["testing", "docs"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will implement explicit `fern-rs test --doc` execution of fenced Fern examples from parser-owned documentation. Trailing `# =>` expectations are checked Fern patterns, including Result/Option wildcards, and failures produce a nonzero test result.
* **Context**: The existing Python documentation check compiles snippets but strips their expected results and never executes them. DESIGN requires runnable examples, multiline setup and constructor-pattern expectations. Reusing the parser, checker and native backend preserves ordinary Fern semantics and gives examples access to private declarations in their owning module.
* **Consequences**: Each example receives isolated local bindings and a checked synthetic test function, while module imports resolve through an in-memory overlay. Library checking validates all bodies without inventing main; ordinary executable checking still requires an entry. Original entry points are preserved by function ID, and validated IR entry selection runs only the test harness. Expectations attach only to complete top-level expression statements, with lexical comment ranges distinguishing markers from string text. Discovery, example count/source bytes, runtime duration and captured output are bounded; current source is never overwritten. General unit-test syntax, coverage and watch mode remain separate CLI milestones. Tests explicitly execute user code; documentation generation itself never executes examples. The unavailable `/decision` skill is replaced by the established decision format.
