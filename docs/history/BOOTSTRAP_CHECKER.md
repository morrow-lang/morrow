> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

> Historical document, retained for architectural context. Its implementation paths, commands and measurements describe the retired setup. See [the current build guide](../../BUILD.md) and [roadmap](../../ROADMAP.md).

# Fern-native quality checker

The native checker runs build, test, examples, style and advisory Git checks using
literal subprocess arguments with explicit time/output limits. Both C and Rust
can compile `scripts/check_style.fn`; `mise run style-fern` runs its full workflow.
`mise run style`, `mise run style-lenient`, `mise run pre-commit` and the primary `mise run check`
workflow use the [native launcher](NATIVE_STYLE_CHECKER.md). Ordinary style checks
require neither Python nor Cargo. Full checks retain explicit Python integration
tests, and the Python checker remains the compatibility reference. The workflow
CLI oracle pins Python 3.14 through its uv script metadata: argparse short-help
cluster handling differs in Python 3.11, so host Python must not silently select
different expected behavior. Normal Python checker execution remains available
under its existing supported versions. Native CLI behavior is independent of
the installed Python version.

`mise run style-parity` compares exact style diagnostic identities, severities,
messages, counts and exits on five fixtures and every compiler/library C source,
then checks the full command workflow. `mise run check` includes the workflow cases,
and `mise run rust-check` repeats diagnostic/workflow checks with the Rust compiler.

The 66 workflow cases cover build warnings/failures, test counts, sorted literal
example paths, bounded error details, missing tools/directories, advisory Git
checks, CLI flags/abbreviations, summary mode and src-before-lib discovery.
Argument failures write stderr and exit 2, including failure of the diagnostic
write itself; help writes stdout. All subprocess calls use the
[bounded process API](../PROCESS_EXECUTION.md) with a 300-second timeout and separate
8 MiB stdout/stderr caps. Process failures produce portable error messages.
Normal tool output, diagnostic records and command ordering remain checked against
the Python reference; terminal decoration and OS-specific exception wording are
not byte-for-byte compatibility promises.

Negative-path classification now matches the pinned Python 3.14 parser: the first
scalar after `-` or `-.` must be a Unicode decimal digit, with arbitrary suffixes
allowed. Thus `-1.c`, `-١abc` and `-.١tail` are positional; superscripts and other
numeric categories are not decimal digits. Nineteen reference-first cases cover
these prefixes, supplementary digits, actual files and explicit `--` termination.
The [Unicode classifier](../STRING_DECIMAL.md) pins the same Unicode 16 profile.

Default migration is verified through cold-build, source/compiler/runtime and
external-dependency invalidation, failure, concurrent invocation and ownership
tests on macOS/Linux. `mise run style-launcher-check` runs the native infrastructure
matrices. See the [launcher contract](NATIVE_STYLE_CHECKER.md) for configuration,
cache cleanup and precise supervision limits.
