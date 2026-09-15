# Morrow Rust style and safety

Morrow-owned implementation and development tools use Rust. Cargo dependencies may
wrap third-party native libraries. Apply these rules to the compiler, runtime,
supervisor, JSON implementation and `xtask`.

## Make invariants visible

Use enums and owned types to represent valid states. Prefer `Result` for fallible
operations and return useful errors at external boundaries. Assertions document
internal invariants; externally supplied malformed input must receive a bounded
error rather than panic. Tests may use assertions and explicit expectations.

Keep functions focused and name the data they consume or produce. Split complex
control flow around meaningful responsibilities rather than arbitrary line
counts. Comment on invariants, ownership and reasons that code alone cannot show.
Public APIs need concise contracts; unsafe APIs need complete safety conditions.

## Bound work and memory

Check input sizes before parsing or allocating. Bound recursive structures,
collection growth, generated output, callback work and resource counts. Loops
should either consume a finite input or enforce an explicit work/deadline budget.
A wall-clock timeout does not replace a deterministic retry or allocation bound.

Use checked arithmetic at byte-size and index boundaries. Preserve full-width
language values across collection storage and the native calling convention.
Do not infer semantic types from register width or spelling.

## Keep unsafe code small

The compiler frontend and lowering use safe Rust. Runtime allocation and POSIX
integration may require unsafe code, isolated behind typed interfaces. Explain
pointer validity, alignment, layout, lifetime, aliasing and thread assumptions.
Match every acquired resource with its owner and cleanup path. Keep roots live
across collector safepoints and avoid accessing native values after shutdown.

## Preserve failures and ownership

Pass literal subprocess arguments. Never manufacture a shell command from source
text or paths. Capture bounded streams and retain child identity until cleanup.
Use exclusive private files and directory-relative operations for publication.
Reject unsafe destinations before replacing existing components. Cleanup must not
follow links, remove another owner's replacement, or hide the original error.

## Verify behavior

Write regressions before implementation. Use independent expected values and
real boundary tests where transport, ABI or resource ownership matters. Prefer
readiness handshakes and injected clocks to guessed sleeps in deterministic tests.
Keep compiler fixtures, native language oracles and failure-path checks readable.

Run focused tests while developing and `cargo xtask check` before committing.
`cargo xtask fmt` and `cargo xtask lint` enforce workspace formatting and lint
policy. Change a lint allowance only with a narrow, documented reason.
