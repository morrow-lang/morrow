+++
schema_version = 1
id = "01M2XHZ8BQVVV17FRV09VJM2NS"
title = "Preserve common CLI controls during Rust migration"
date = "2026-09-06"
status = "accepted"
tags = ["rust", "tooling"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for common output controls, bounded syntax inspection and documentation opening (108A–C)
* **Decision**: I will parse bounded global quiet/verbose/color controls before subcommand dispatch while retaining literal option operands and all run arguments after `--`.
* **Context**: A C/Rust command audit and failing regressions found missing common controls and an incorrect successful status for a missing command. The existing std-only parser can preserve the compatibility contract without a dependency change.
* **Consequences**: Quiet hides check/build/test summaries and interactive prompts; errors, explicit help/version, generated data and native program streams remain visible. Verbose identifies the selected command on stderr. Color applies only to human compiler/test output; auto checks its actual terminal destination and NO_COLOR, while explicit always wins. `-v` aliases version. Bound arguments at 4096 words/1 MiB before dispatch, preserve non-UTF8 operands, and return status 1 for a missing action. Rust emit-output/fmt-check extensions remain. 108B adds source-only lex/parse inspection of the actual Rust token/AST representation. Preserve source/token/depth limits, check a 1 MiB byte read before UTF-8 decoding, and format into a 16 MiB bounded buffer before publication. Dump text is escaped and uncolored, not stable serialization or C byte parity; no module loading, typing or execution occurs. Parser/limit errors publish no partial dump, while a physical output failure cannot roll back already written bytes. 108C makes --open imply HTML, retaining explicit output or fern-docs.html in the current directory before invoking the fixed platform opener with one canonical absolute OS path. Failures remain visible best-effort notes after successful generation. The launcher has null streams and a ten-second polling deadline; timeout stops and reaps only that owned child, with kernel-uninterruptible waits explicitly outside the bound. All documentation modes check raw source bytes against 1 MiB per file and 8 MiB aggregate before UTF-8 decoding. Default installation remains later scope. Source/runtime semantics and the shipping compiler are unchanged.
