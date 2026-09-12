# Rust workspace acceptance

Date: 2026-09-12. Accepted on macOS ARM64 and Linux ARM64. Fern-owned
implementation and tooling are Rust. Both platforms pass the complete debug
quality gate, selected optimized runtime/ABI checks, release-native acceptance
and actual archive/installation relocation checks. The exact scope and artifact
identities are recorded below.

## Implementation scope

| Component | Current implementation |
| --- | --- |
| Compiler and tools | `crates/fern`: parser, checker, typed lowering, Cranelift object backend, CLI, formatter, REPL, documentation and LSP |
| Native runtime | `crates/fern-runtime`: Rust tracing collector, native ABI, collections, strings, JSON codecs, IO, process execution, services, terminal widgets and actors |
| Program startup | `crates/fern-runtime-native`: entry archive, separate from the core library used by ABI tests |
| Shared JSON | `crates/fern-json`: safe bounded parser, conversion, immutable graphs and encoding, shared by native execution and the REPL |
| Test supervision | `crates/fern-test-supervisor`: bounded process capture, retained child identity, group cleanup and shared parent protocol |
| Repository tooling | `xtask`: Cargo artifact staging, acceptance, fuzzing, lint policy, measurement, packaging, installation and uninstall |

The old C compiler/runtime, QBE implementation, Python maintenance scripts,
bootstrap/style launchers and generated Tree-sitter editor integration are
removed. Editor integration uses the Rust language server, `fern lsp`.
Documentation uses static accessible navigation and browser Find; no authored
JavaScript filter remains. Historical decisions and reports retain their original
names and measurements under `docs/history` and `docs/reports`.

All Fern-owned implementation and tooling is Rust. Third-party native code behind
Rust wrappers remains allowed, as the user explicitly requested. SQLite is bundled
through rusqlite; HTTP uses ureq/rustls with ring cryptography. OS interfaces and
the host linker/SDK remain necessary. The [third-party notices](../THIRD_PARTY_NOTICES.md)
record the enabled dependency closure, license texts and Unicode provenance.
This does not promise that every transitive dependency or resulting executable
is entirely Rust or universally static.

## Executed checks

Both hosts use the pinned `nightly-2026-09-06` toolchain. The Linux host is an
Ubuntu 26.04 ARM64 Lima guest with four virtual CPUs and 6 GiB of memory.

| Gate | macOS ARM64 | Linux ARM64 |
| --- | --- | --- |
| Full debug quality command | Passed | Passed |
| Standard workspace test cases | 1,752 passed | 1,754 passed |
| Additional custom IO/PTY cases | 5 passed | 5 passed |
| Independent native expected-output fixtures | 305 passed | 305 passed |
| Public example typechecks | 18 passed | 18 passed |
| Dynamic compatibility programs | 63 passed | 63 passed |
| Atomic rejection fixtures | 295 passed | 295 passed |
| Continue after a failed source unit test | Passed | Passed |
| Default grammar/mutation fuzz | 64 + 192 passed | 64 + 192 passed |
| Optimized runtime/JSON/supervisor tests | 92 + 5 custom IO/PTY cases passed | 92 + 5 custom IO/PTY cases passed |
| Separate optimized object/ABI/GC tests | 12 passed | 12 passed |
| Compiler phase benchmark fixtures | 3 passed | 3 passed |
| Compiler phase smoke cases | 10 passed | 10 passed |

The quality command includes formatting, Clippy with warnings denied, workspace
unit/integration/doc tests, native fixtures, compatibility cases and deterministic
fuzzing. The lint contract itself verifies 18 forbidden snippets and two accepted
cases with the pinned toolchain. Distribution tests cover 26 bounded archive,
installation and uninstall contracts. Compiler tests exercise the real LSP and
REPL terminal processes; the custom runtime harness checks terminal and descriptor
behavior. Linux's nested capture-test re-execution prints an additional one-test
summary; it is not counted again as a distinct test.

The optimized checks target the unsafe and native boundaries: collector roots,
interior pointers, cycles, allocation pressure, finalized Rust graphs, process
cleanup, terminal restoration, full-width payloads, mixed register/stack arguments,
exact NaN transport and a live Cranelift pointer across collection. This record
does **not** claim a completed optimized run of every compiler integration test.
The complete compiler suite was run in the debug quality gate.

Both hosts additionally passed the release-staged 305 native fixtures, complete
compatibility cases and extended 512-case grammar corpus with 192 retained
mutations. The grammar seed is `0xC0FFEE`; mutation
seed is `0xFE12A`. Compiler phase smoke used the release profile on macOS and
the development profile on Linux; neither smoke run is a comparative timing result.

## Distribution and measurements

The release layout contains `fern`, `fern-test-supervisor`, `libfern_runtime.a`,
`fern-package.json`, the license notices and optional README. The marker identifies
format 2, Rust compiler/runtime and Cranelift. Installed components remain together
in `bin`, with documentation in `share/fern`.

Packaging checks exact member names, regular-file types, modes, lengths, complete
tar/gzip framing and SHA-256 before publication. Installation opens directory
components without following links, copies completed private files and replaces
each component atomically. Uninstall removes only known names after preflight.
An installed marker disables implicit fallback to a development checkout.

Both final archives passed relocation to paths containing spaces, native
build/run, exact source-test output through the packaged supervisor, installation
and safe uninstall. Linux additionally verifies explicit failure for a missing
packaged runtime without checkout fallback or output publication. These reports
identify each tested archive, component hash, file mode and observable result:

- [macOS archive and installation](reports/rust-runtime-release-archive-smoke-macos-arm64-2026-09-12.json)
- [Linux archive and installation](reports/rust-runtime-release-archive-smoke-linux-arm64-2026-09-12.json)
- [macOS performance measurements](reports/rust-workspace-performance-macos-arm64-2026-09-12.json)
- [Linux performance measurements](reports/rust-workspace-performance-linux-arm64-2026-09-12.json)

The archive and performance reports identify the same compiler, runtime and
supervisor bytes on each host. The final macOS `cargo xtask package` also
reproduced the verified archive byte for byte.

| Measured release artifact | macOS ARM64 | Linux ARM64 |
| --- | ---: | ---: |
| Compiler | 5,964,016 bytes | 5,903,432 bytes |
| Runtime archive | 13,322,176 bytes | 21,767,898 bytes |
| Supervisor | 358,000 bytes | 332,736 bytes |
| Distribution archive | 7,362,589 bytes | 8,275,826 bytes |
| Compiler startup median, 30 samples | 1.619 ms | 0.358 ms |
| Compiler startup p95, 30 samples | 1.985 ms | 0.535 ms |

Performance must be read with its artifact hashes, host and measurement method.
The former 4 MiB compiler ceiling measured a frontend without embedded Cranelift;
it is not a valid measurement of this implementation. Warm startup and bounded
native application samples do not establish cold-build or cross-machine performance.
The macOS application samples show substantial scheduling variance. Linux
generated executables retain debug/symbol sections under the verified linker
settings; these are observed artifact sizes, not minimum-size claims.

## Scope that remains open

This completes an implementation migration, not every proposal in the language
vision or a 1.0 release. [Release readiness](RELEASE_READINESS.md) retains the open
language and product gates: generalized actor suspension and typed supervision,
actor REPL parity, deeper JSON union discrimination, custom traits, HTTP serving,
typed SQL queries, Sets, advanced ownership/reuse analysis and a WASM backend.
Unsupported forms must continue to produce explicit diagnostics.

The collector has platform implementations for ARM64 and x86-64 on macOS/Linux.
Only ARM64 execution was accepted here; each additional architecture needs its
own runtime, ABI, process and distribution validation before publication. Full
compiler optimization-mode coverage, sanitizer coverage, source debugger support
and controlled cross-architecture performance are not implied by these results.
No tag, registry publication or public release is authorized by this report.

## Reproduce

```sh
cargo xtask check
cargo xtask build --release
cargo test -p fern-runtime -p fern-json -p fern-test-supervisor --release
cargo test -p fern --release --test cranelift_backend
cargo xtask native
cargo xtask compatibility
cargo xtask fuzz 512 0xC0FFEE
mise run rust-bench-smoke
cargo xtask perf report.json
cargo xtask package
```

Use the [build guide](../BUILD.md) for installation and archive verification.
The previous compiler-default checkpoint is preserved in
[the historical migration report](history/RUST_DEFAULT_MIGRATION.md) and must not
be substituted for this runtime/backend evidence.
