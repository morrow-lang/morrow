# Deterministic compiler fuzzing

Run `cargo xtask fuzz 512 0xC0FFEE`. The first seven cases use the retained
`corpus/*.mr` seeds; subsequent cases use the Rust grammar generator. Each case
must parse, format twice without changing, and parse again. Every subprocess
runs through the bounded Rust test supervisor. Failures print the seed, index,
stage and original source for reproduction.

`generated-c0ffee.json` records all 512 original generator outputs, including
the first seven generated cases that the runner replaces with seed files. The
Rust generator was compared byte for byte against the former generator before
its removal. Its wrapping xorshift arithmetic and RNG draw order are stable.

Every invocation also executes the 192 preserved mutation sources in
`tests/fixtures/fuzz-mutations.json` (seed `0xFE12A`). Invalid input must fail
without a crash; successful checking must lower; failed formatting must leave
the input unchanged; successful formatting must be idempotent and preserve
lowering behavior. These fixtures retain the previous acceptance sample
without requiring Python or a second compiler implementation.
