# Morrow contributor and agent guide

The repository owns a Rust compiler, Cranelift backend, Rust runtime, language
server, native test supervisor and Cargo-based development tooling. Read
[DESIGN.md](DESIGN.md), [ROADMAP.md](ROADMAP.md), [decisions/](decisions/) and
[MORROW_STYLE.md](MORROW_STYLE.md) before changing behavior. The design includes planned
features; the roadmap records implementation and acceptance evidence.

## Work test first

1. Define the observable behavior and write an independent regression test.
2. Run it and record the expected failure.
3. Implement the smallest complete change that satisfies the contract.
4. Run focused tests, then `cargo xtask check` before committing.
5. Update the roadmap immediately when a task is verified. Document significant
   architectural choices and limitations in the decision record.
6. Commit a coherent change with a conventional message explaining its purpose
   and validation.

Do not remove, skip or weaken a failing oracle to make a gate pass. If a test
relies on timing, ambient caches or a replaced implementation, preserve its
behavioral assertion while making its setup deterministic. Backend/runtime
changes require independent expected-output and ABI tests, not only agreement
between implementations sharing the same lowering.

## Commands

```sh
cargo xtask build
cargo test -p morrow --lib
cargo test -p morrow-runtime
cargo xtask test
cargo xtask check
cargo xtask build --release
cargo xtask package
```

`cargo xtask fmt` formats Rust. `cargo xtask lint` checks formatting and Clippy.
Combined acceptance commands prepare the compiler, runtime and supervisor before
running native tests. Direct `native` and `examples` commands use staged `bin/`
components. Root `rust-toolchain.toml` and `Cargo.lock` define the supported build.

## Ownership and safety

- Prefer safe Rust ownership, enums, `Result`, slices and standard collections.
- Keep `unsafe` in narrow, documented ABI/OS boundaries. State pointer validity,
  layout, lifetime, thread and ownership requirements for each unsafe operation.
- Morrow code generation uses validated typed machine IR and Cranelift objects.
  Textual IR output is for inspection; native builds require no external IR compiler.
- Native values use the Rust runtime's fixed ABI and collector. Preserve full-width
  payloads, callback signatures and allocation roots across safepoints.
- Keep explicit limits on external inputs, recursion, output, work and resources.
  Error paths must preserve cleanup and the original failure.
- Pass subprocess arguments literally. Own temporary directories exclusively;
  publish completed files atomically and never traverse foreign replacement links.
- Supervised child identity remains retained until group cleanup and reaping.
  Do not send signals to a PID after surrendering ownership of that identity.
- Do not add Morrow-authored setup implementations in another programming language.
  Cargo dependencies may wrap third-party native libraries when needed.

## Scope and verification

Keep changes focused and coordinate shared-file ownership during parallel work.
Do not clean another task's build outputs. Use separate Cargo targets for isolated
experiments when necessary; account for their disk usage. Avoid repeated broad
builds after a complete passing gate unless a later change warrants them.

Document current limitations honestly. Preserve dates, original component names
and measurement context in historical reports. A new backend/runtime needs new
acceptance evidence; old reports remain historical evidence.

The compiler provides editor support through `morrow lsp`. Its own parser is the
language source of truth. Do not introduce a separate generated editor grammar
or another parser toolchain as part of a routine compiler change.
