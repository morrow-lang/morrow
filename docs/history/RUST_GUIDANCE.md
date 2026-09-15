> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

> Historical document, retained for architectural context. Its implementation paths, commands and measurements describe the retired setup. See [the current build guide](../../BUILD.md) and [roadmap](../../ROADMAP.md).

# Rust guidance adoption (Decisions107,111 and112)

Fern uses **nightly-2026-09-06**, safe owned representations and a
standard-library-only default compiler. Decision112 adds optional pinned
Cranelift code-generation dependencies and a dedicated feature acceptance gate.
Decision111 supersedes the earlier Rust 1.75
preservation policy at the user's request; the numeric Cargo floor is 1.100,
with no stable MSRV promise. Edition 2021 remains unchanged. The original
Decision107 checkpoint added strict incremental lint checks and an independently
locked Criterion developer workspace, fixing a directory-discovery boundary and
unnecessary allocations. Historical test and benchmark results below retain
their original toolchains; they do not establish nightly validation. See
[the roadmap](../../ROADMAP.md) for current verification evidence.

## Verification and development workflow

The mise migration in Decision106 owns tool provisioning, CI, watchers and
nextest. Use [Development tasks and tools](../DEVELOPMENT_ENVIRONMENT.md) for exact
pins and installation boundaries; no Nix/devenv or replacement Justfile is added.
Decision107 contributes `rust-lint-policy`, `rust-bench-smoke` and optional
`rust-bench` tasks. The underlying commands also work directly:

```sh
python3 scripts/test_rust_lint_policy.py
cargo fmt --manifest-path compiler-rs/Cargo.toml --all -- --check
cargo check --manifest-path compiler-rs/Cargo.toml --locked --all-targets --all-features
cargo clippy --manifest-path compiler-rs/Cargo.toml --locked --all-targets --all-features -- -D warnings
cargo test --manifest-path compiler-rs/Cargo.toml --locked --all-features
cargo test --manifest-path compiler-rs/Cargo.toml --locked --doc
cargo bench --manifest-path benchmarks/compiler-phases/Cargo.toml --locked --bench phases -- --test
```

The original 2026-09-06 Decision107 checkpoint ran these on Rust 1.75, plus the benchmark
package's tests/Clippy, offline build and an actual statistical baseline.
Decision106 separately verified nextest, Bacon check/Clippy and a watchexec
source-change dispatch; nextest does not replace documentation tests.
The combined macOS/Linux integration also passed the complete Rust/native/C
quality gates, native cache/ownership suite and documentation checks. Nextest ran
1409 tests on macOS and 1410 on Linux, all passing without skips; three Rust
documentation tests ran separately. Linux provisioned the pinned mise/Python/uv
tools and rust-src successfully. The workflow gate includes 23 task, literal-flag
and script-lock checks; Criterion fixtures and all 10 smoke phases pass on both.

The tools also caught regressions during the subsequent actor integration: paired
Criterion measurements exposed redundant cloning and validation during QBE
emission. Reusing already validated immutable data removed 77–86% of that added
phase cost while preserving all original public-IR checks. Non-actor emitted bytes
match the earlier compiler. The [measurement record and limits](../../benchmarks/compiler-phases/README.md#actor-preparation-review)
include remaining cost, host contention and independent malformed-IR/native fault
audits. Later gate counts are tracked in [the current roadmap](../../ROADMAP.md).

## Enforced lint policy

Cargo's package lint table provides the current enforcement contract. [Cargo reference](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section)

| Scope | Policy and evidence |
| --- | --- |
| Entire compiler package | Deny `dbg_macro`, `todo`, `unimplemented`, `exit`, `unchecked_time_subtraction`, `unused_peekable`, `redundant_clone`, `or_fun_call`. All-target Clippy is a required gate. |
| Production library and binary | Deny `panic` and `panic_in_result_fn` via `cfg_attr(not(test), ...)`; intentional test assertions/panics remain available. |
| Source-directory, native linker-argument and test-frame boundaries | Deny `pedantic`, `nursery`, `unwrap_used`, `expect_used`, `indexing_slicing`, `as_conversions`, `unreachable`, `string_slice`, `arithmetic_side_effects`. These are audited boundary modules, not a claim of whole-compiler compliance. |
| Narrow allowances | Discovery counter/depth arithmetic has explicit small bounds (8193 and 33); one documented function allowance preserves those validated operations. The frame decoder has a documented small-offset allowance (header under 128 bytes and two streams up to 256 KiB); explicit caller visibility has a separate `redundant_pub_crate` allowance. A local `large_enum_variant` allowance preserves the public statement IR representation pending a separate allocation/layout audit. No crate-wide group allowance masks failures. |

The policy test compiles 18 deliberately bad, dependency-free temporary crates
under the selected toolchain and checks each expected diagnostic, then compiles
a checked positive and a test-only panic without executing either fixture. It rejects weakened manifest entries and unsupported lint
names. It uses offline Cargo and does not execute the panic/exit fixtures.

The nightly policy uses `unchecked_time_subtraction`, the current name for
`unchecked_duration_subtraction`, and tests both `Instant - Duration` and
`Duration - Duration`. The original Rust 1.75 checkpoint tested the predecessor
name for `Instant - Duration` only. [Clippy lint documentation](https://rust-lang.github.io/rust-clippy/master/index.html#unchecked_time_subtraction)
Modern Clippy supports `allow-panic-in-tests`; the project retains its explicit
production `cfg_attr` restrictions for both panic lints instead. This preserves
the existing compile-tested scope without adding redundant configuration.
[Clippy test configuration](https://doc.rust-lang.org/clippy/lint_configuration.html#allow-panic-in-tests)

A broad audit found over a thousand arithmetic warnings and hundreds of indexed
accesses, many in bounded IR/ABI traversal. Enabling every restriction globally
would require a separate invariant audit, rather than mechanically replacing
validated accesses or adding blanket allowances. The next modules should follow
the same rule: capture a real failing boundary test, fix the defect, and enable
the restriction with narrowly justified exceptions.

The directory pilot caught a real defect: a starting path over 4096 bytes was
copied and passed to filesystem lookup before the child-path limit applied.
It now rejects that input before copying or formatting an oversized OS error.
Other fixes remove unnecessary clones, eagerly created fallback strings/errors,
and a peekable iterator that never peeks. Existing semantic regressions remain
unchanged except removal of redundant test clones required by the same policy.

## Tools and API choices

| Recommendation | Decision and concrete reason |
| --- | --- |
| rustfmt, Clippy, unit/integration/doc tests, CI | Retained and exercised; mise provides focused and aggregate tasks. Strict warnings remain the enforcement boundary. |
| Rust Analyzer | Documented editor setup in Decision106; no user-editor installation. rust-src is a separate managed component, not a language server. |
| Bacon | Optional project-local tool, built with its separate 1.98.1 toolchain; its check/Clippy jobs select Fern's dated nightly. Decision106 originally tested jobs on 1.75. It provides an interactive view beyond the simple watcher. |
| cargo-nextest | Optional pinned prebuilt runner using Fern's selected toolchain; original Decision106 verification used 1.75. No retries conceal failing tests; doctests remain a separate command. |
| watchexec | Optional bounded check workflow, source-change behavior tested. It does not launch arbitrary Fern programs or alter dependencies. |
| Criterion | Added as a dev-dependency only in `benchmarks/compiler-phases`; pinned 0.5.1 and its own lock originally verified on 1.75, 10 real phase measurements and fixture oracles. [Methodology](../../benchmarks/compiler-phases/README.md) |
| cargo-generate | Not installed: this repository maintains one compiler and no reusable starter-project template. There is no current scaffolding operation to exercise or validate. Revisit when a Fern application/extension template exists. |
| cargo-seek | Not installed: this change has one known developer dependency, verified against its official manifest and real locked builds. No ongoing interactive crate-discovery workflow justifies another executable; reconsider during actual dependency selection. |
| Typestate | Existing private probe/editor/codec-template tokens and publication validators express meaningful construction/execution boundaries and have compile-fail tests. No generic state wrapper is added to ordinary mutable checker state without an operation it would make impossible. |
| Nightly | Adopted by user request in Decision111, pinned to nightly-2026-09-06 through mise and rustup. Upgrades require renewed gates. It does not itself implement Cranelift or require unstable language features. |
| Git hooks | Optional convenience; CI/task checks enforce the contract. No automatic hook installation or dependency updates on entering the environment. |

## Crate selection

The absence of a production dependency is a scoped capability decision, not a
rule against libraries. Reassess MSRV, maintenance, feature cost and actual API
fit when the capability is introduced. This checkpoint makes no unsupported
claim that an omitted crate is unsafe, unmaintained or intrinsically unsuitable.

| Suggested crate | Current decision |
| --- | --- |
| `color-eyre` | Existing public `Diagnostic` values and exact CLI/LSP source ranges must stay structured. No new application error-reporting boundary needs a second report type. |
| `itertools` | The audited operations use direct standard iterators; no awkward custom iterator algorithm was found that this change would replace. |
| `rayon` | No measured parallel workload is being implemented. Shared checker budgets/order and small fixture overhead need design and measurements before parallel execution. Criterion default Rayon is disabled too. |
| `serde` + derive | Fern's wire JSON codec operates on Fern types and native/REPL representations with exact number/error/resource contracts; Rust derive does not implement that contract. Criterion uses serde transitively for its developer reports only. |
| `clap` + derive | CLI parity has an independent Decision108 scope. Changing parser diagnostics/streams/exit behavior here would break that boundary. Criterion uses its own locked CLI dependency in the developer package. |
| Chrono / Jiff | Compiler timing uses monotonic `Instant`; no calendar, timezone or date parsing API needs either crate. |
| `cmd_lib` | Existing process capture has tested argv identity, deadlines, aggregate output limits, process ownership and cleanup. No demonstrated benefit warrants replacing it with shell-oriented execution. |
| `utoipa` | There is no Rust HTTP/OpenAPI service to describe. LSP JSON-RPC is not an OpenAPI endpoint. |
| `reqwest` / rustls | Compilation and LSP have no new outbound HTTP operation. Native Fern runtime networking does not make the compiler itself an HTTP client. |
| `sqlx` | The compiler does not query a database or own migrations. Runtime SQLite bindings are a different layer. |
| Leptos / Trunk | No Rust web frontend is being built. Editor grammar WASM and test drivers are not a web application. |
| Dioxus | No cross-platform application UI is part of this compiler checkpoint. |
| Tauri | Existing Zed extension/LSP packaging has its own host; introducing another desktop shell would not improve that integration. |

The production Cargo manifest and lockfile remain dependency-free. Criterion's
lock is reviewed independently and default plotting/parallel/async features are
disabled. No credentials, service defaults, production URLs, global installs or
automatic dependency updates are introduced.

## Remaining scope

Broader pedantic/restriction coverage requires more bounded module audits.
Statistical phase measurements need quiet-host before/after runs before any
performance claim; smoke tests alone prove operation, not speed. The baseline
records a development run with possible concurrent host activity. Backend
replacement evaluation, CLI parity, actors, recursive Result proofs and future
JSON features retain their own tests and decisions. No optional tool installation
or platform test that was not actually run is counted as completed here.

## Native boundary follow-up

The strict module policy now also covers literal linker-argument parsing and
native test-frame decoding. The linker parser rejects NUL before constructing
argv, bounds input to 65,536 bytes, words to 16,384 UTF-8 bytes and output to
4,096 arguments, and preserves non-ASCII whitespace in literal Unix paths.
Only unquoted ASCII space/tab/newline delimit words. Existing shell-compatible
double-quote escapes, quoted whitespace and empty arguments remain unchanged.
Late malformed input returns an error without publishing a partial argv.

These are parser allocation/work limits; the existing `pkg-config` subprocess
still uses `Command::output`. This follow-up does not claim to impose a deadline
or bounded stream capture on that external producer. The mise Bash decoder and
native-bootstrap metadata decoder have their own explicitly documented quoting
contracts and are not silently substituted for the linker parser.

The frame audit found no status or wire-layout defect. It replaces indexing and
an unchecked cast with guarded access and checked conversion, retaining a narrow
arithmetic allowance: header length is below 128 and each stream is at most
256 KiB, so payload offsets fit even a 32-bit `usize`. Tests cover all 256 normal
exit codes, exact stream limits, binary payloads containing protocol markers,
truncated/trailing records and extreme declared lengths/status values. Process
ownership, reaping, timeout transport and the fault ABI remain unchanged.


The GC archive lookup separately treats successful `pkg-config --variable=libdir`
output as one literal UTF-8 path, not as linker words. It removes only one terminal
LF or CRLF, preserves ASCII and non-ASCII whitespace (including a space-only
folder name), and rejects empty, NUL, multiline, invalid UTF-8 or over-4,096-byte
paths before filesystem lookup. At most 4,098 input bytes can reach this decoder
when CRLF framing is included; the path itself is limited to 4,096 UTF-8 bytes.
Quotes, backslashes and shell-looking bytes stay literal. Failed or unavailable
`pkg-config`, and a valid directory with no static archive, retain the established
linker-flag fallback. Malformed successful metadata reports an error instead of
probing the current directory, selecting a replacement-character path or falling
back silently. The leaf decoder uses the same strict lint policy as linker words.
Tests invoke fake tools in child processes, use real archive/decoy paths and verify
exact linker argv without changing the test runner's environment. These bounds
apply after `Command::output` completes; no producer capture quota or deadline is
claimed.
