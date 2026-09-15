> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

> Historical document, retained for architectural context. Its implementation paths, commands and measurements describe the retired setup. See [the current build guide](../../BUILD.md) and [roadmap](../../ROADMAP.md).

# Rust default compiler migration

The build and installation select the safe Rust compiler as `fern`. `fern-c`
retains the C implementation for bootstrap and explicit legacy-source/ABI
regressions. Both report the same release version. QBE is the default backend;
Cranelift remains an opt-in build of the same typed machine lowering.

## Acceptance contract

- The [212-name language inventory](../LANGUAGE_PARITY.md) covers the executable C
  source baseline and intentional corrections. Parser-only planned syntax and
  known C miscompilations are not compatibility oracles. Rust adds full-width
  values, checked generic definitions, modules, typed JSON codecs and bounded actors.
- [Result handling](../RESULT_HANDLING.md) requires proof of handling or complete
  transfer. Recursive builder contracts may retain duties but never grant
  provisional consumption credit. Unsupported proof equations reject explicitly.
- [Tooling parity](../TOOLING_PARITY.md) covers public CLI controls, literal paths,
  documentation, source tests, terminal REPL and actual versioned LSP edits.
- Installation and release archives require `fern`, `fern-c`, `fern-qbe`,
  `fern-test-supervisor`, `libfern_runtime.a`, the package marker, project license
  and third-party notices. Missing installed helpers cannot borrow checkout files.
- macOS and Linux acceptance requires the full quality, Rust/QBE, Cranelift,
  documentation, fuzz, installation, packaging and separate compiler performance
  gates. All passed on 2026-09-12; the results below close this contract.

## Completed acceptance: 2026-09-12

The [validation ledger](../reports/rust-default-validation-2026-09-12.json) records
successful commands and log digests for macOS ARM64 and Linux ARM64. Both tested
the same source and test tree, SHA-256
`15f79384e71bfaabd6d4b15e985d23975de4799a1759aaae425026bb5cd3e26d`.
Measurements preceded the final documentation/default-switch commit; the raw
reports retain their actual commit and dirty-worktree metadata.

| Acceptance gate | macOS ARM64 | Linux ARM64 |
| --- | ---: | ---: |
| Rust/QBE Cargo tests | 1,621 passed | 1,624 passed |
| Cranelift Cargo tests | 1,633 passed | 1,636 passed |
| Cranelift native corpus / migration programs | 305 / 10 passed | 305 / 10 passed |
| C reference tests / checked examples | 574 / 18 passed | 574 / 18 passed |
| Workflow/style parity cases | 66 passed | 66 passed |
| Differential fuzz smoke, each frontend | 64 passed | 64 passed |
| Installation checks | 9 passed | 9 passed |
| Extracted release archive checks | 10 passed | 10 passed |

The full gates also cover runtime/ABI checks, native debug/release/sanitizer
profiles, actor protocol mutations, documentation, LSP RPC, bootstrap cache and
permission checks, benchmark smoke and lint policy. Final test-fixture fixes
removed executable-opening and wall-clock races without weakening assertions;
the ledger identifies the affected suites rerun after those changes. An additional
512-case differential run passed with each frontend on macOS.

Actual release archives were extracted outside the checkout into paths containing
spaces and shell punctuation. Checks covered versions, typed checking, native
build/run, source tests, and rejection of missing packaged QBE/runtime/supervisor
components despite a usable checkout. Archive reports preserve source fixtures,
outputs and component digests:
[macOS](../reports/rust-default-release-archive-smoke-macos-arm64-2026-09-12.json),
[Linux](../reports/rust-default-release-archive-smoke-linux-arm64-2026-09-12.json).
The archives and checksums are generated locally under `dist/`; they have not
been published.

## Release performance

| Measurement | macOS ARM64 | Linux ARM64 | Ceiling |
| --- | ---: | ---: | ---: |
| Combined release build | 44.21 s | 44.01 s | 150 s |
| Rust compiler size | 3,823,744 bytes | 3,806,232 bytes | 4,194,304 bytes |
| C reference size | 548,488 bytes | 651,976 bytes | 1,500,000 bytes |
| Rust startup p95 | 6.62 ms | 0.41 ms | 100 ms |
| C reference startup p95 | 2.58 ms | 0.32 ms | 100 ms |

All absolute budgets passed. Measurements ran sequentially on one physical host:
macOS had 10 logical CPUs and default Cargo concurrency; the Linux VM had four
vCPUs and two Cargo build jobs. Native outputs were cleaned and the selected
host-target release directory did not exist before each measurement. Downloaded
dependencies and other Cargo caches were retained; CPUs were not isolated.
These are observations from these environments, not cross-platform rankings.

The expanded typed Rust frontend took longer than C on the synthetic frontend
workload. Reports include 30 check/emit samples and 10 native build samples per
compiler, with verified executable output:
[macOS frontend evaluation](../reports/rust-default-frontend-evaluation-macos-arm64-2026-09-12.json),
[Linux frontend evaluation](../reports/rust-default-frontend-evaluation-linux-arm64-2026-09-12.json).
Three real example case studies record startup and generated-program timings:
[macOS case studies](../reports/rust-default-benchmark-case-studies-macos-arm64-2026-09-12.md),
[Linux case studies](../reports/rust-default-benchmark-case-studies-linux-arm64-2026-09-12.md).

## Runtime and future language work

This completes the compiler migration contract, not every future feature in
DESIGN.md. The shared runtime, native supervisor, QBE and editor parser remain
native components. Host C/link tools and GC, SQLite and OpenSSL development
libraries remain required. Generalized concurrency, richer server/database APIs,
full editor grammar and a 1.0 release are tracked separately in
[release readiness](../RELEASE_READINESS.md).

## Reproduction

Use the pinned mise environment and native dependencies in [BUILD.md](../../BUILD.md).
Run `mise run check`, `mise run rust-check`, `mise run rust-cranelift-check`,
`mise run docs-check`, `mise run fuzz-smoke`, `mise run style-parity`,
`mise run lsp-rpc-smoke`, `mise run release-policy-check`, `mise run perf-budget`,
`mise run release-package` and `mise run release-package-check` sequentially on
both platforms. Build tasks share native output directories.

Performance ceilings remain 150 seconds for the release build and 100 ms for
startup p95. The explicit C reference retains its 1,500,000-byte compiler ceiling;
the expanded Rust frontend has a separately enforced 4 MiB ceiling (Decision118).
These compiler measurements exclude helpers/runtime and do not claim generated
program sizes or universal performance.
