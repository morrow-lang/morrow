+++
schema_version = 1
id = "01M2XHZ82YS3CY7P62AAYR5NQS"
title = "Expose safe constant arithmetic and retain integer tail parameters in SSA"
date = "2026-09-14"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; independent lowering, native arithmetic, seeded tail-state and paired performance checks pass
* **Decision**: Emit direct native integer division/remainder when the lowered divisor is a known nonzero literal other than `-1`, letting Cranelift apply its existing exact strength reduction. Preserve the helper for zero, `-1` and nonliteral operands. For already eligible direct self-tail functions with all-Int parameters, use typed phi parameters and simultaneous backedge updates after complete source-order argument evaluation. Finalize incoming edges before entry-allocation insertion. Keep managed/mixed parameter storage and capture/defer eligibility unchanged.
* **Context**: The arithmetic benchmark hides its constant divisor behind a general helper and repeatedly stores loop parameters in memory. Constant arithmetic alone is slower on the measured M4; exposing the arithmetic and retaining loop state in registers together reduces 20-million-step elapsed time from 76.60 to 66.57 ms, against Rust's 57.35 ms. The SSA-only control confirms both changes are needed for this measured gain.
* **Consequences**: Integer wrapping, zero faults, operand effects, cleanup and root lifetimes remain required. Independent i128 arithmetic checks cover 113,508 outputs; seeded native recurrence checks cover full-width permutations, multiple backedges, deep iteration and failure/collection paths. No per-iteration call, division or parameter-memory traffic remains in the measured scalar loop. Mixed/managed loops, ordinary non-tail calls, floating-point semantics and WASM retain their existing paths. Dynamic arithmetic and remaining backend instruction selection need separate measurement. See `benchmarks/language-comparison/ARITHMETIC.md`, including the unsuccessful first experiment.
