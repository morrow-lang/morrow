+++
schema_version = 1
id = "01M2XHZ97ZESQD06KVS93HZGF4"
title = "Compiling to C via QBE"
date = "2026-01-26"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Adopted
* **Decision**: I will compile Fern to C using `QBE` as an intermediate representation.
* **Context**: I considered three approaches: (1) `LLVM` - powerful but extremely complex with 20+ million lines of code and steep learning curve, (2) Direct machine code generation - too low-level and platform-specific, requiring separate backends for each architecture, (3) `QBE` - a simple SSA-based IL that compiles to C. QBE hits the sweet spot: it's only ~10,000 lines of code (AI can understand it fully), generates efficient C code, handles register allocation and optimization, and lets me target any platform C supports. The C output can then be compiled with any C compiler (gcc, clang, tcc) for maximum portability.
* **Consequences**: I need to generate QBE IL from Fern AST, then invoke QBE to produce C code, and finally compile the C code to native binaries. This adds an extra compilation step but dramatically simplifies the compiler implementation and ensures broad platform support. Single binaries under 1MB are still achievable.
