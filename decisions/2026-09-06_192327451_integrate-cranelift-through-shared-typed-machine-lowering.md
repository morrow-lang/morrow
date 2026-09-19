+++
schema_version = 1
id = "01M2XHZ8AVSS58V25A1SR3FHR0"
title = "Integrate Cranelift through shared typed machine lowering"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for implementation and opt-in acceptance testing; default promotion requires the backend gates
* **Decision**: I will extract the existing semantic lowering into a bounded typed machine representation, render QBE from it, and emit native objects with pinned Cranelift0.135.1 from the same representation. This supersedes the standard-library-only dependency restriction for the optional Cranelift backend, not for unrelated compiler code.
* **Context**: The user requested completing the Cranelift integration. The isolated scalar experiment does not exercise Fern's closures, cleanup, runtime ABI or actors. Translating emitted QBE text would retain the coupling and introduce another language parser. Canonical runtime signatures must retain actual result widths even when callers discard or narrow values; Float formatting needs fixed-signature runtime entry points.
* **Consequences**: Keep safe Rust at the compiler boundary, dated nightly and locked dependencies. Preserve independent expected-output tests and QBE as the reference until full native, GC, ABI, debugger and performance acceptance. Cranelift replaces the C code generator, not the C runtime, external libraries, native supervisor or generated editor parser. Python correctness oracles remain developer tooling. Record precisely which gates pass before changing defaults; do not equate an integrated backend with a Rust-only runtime.
