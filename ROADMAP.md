# Fern Roadmap

Last updated: 2026-09-12

This file is the only active roadmap. Historical context is in [`docs/HISTORY.md`](docs/HISTORY.md).

## Current Status Snapshot

- Quality gate: `mise run check` passing (574 C tests, 16 full-width Int programs, native file/process/stderr/workflow/string/print tests, 13 TUI tests, 18 examples, strict style); validated with stale host `LIBRARY_PATH` excluded
- Perf gate: `mise run perf-budget` passing on macOS arm64 (4.39 s build, 548,488-byte compiler, 3.92 ms startup p95; 2026-09-12)
- Fuzz gate: `mise run fuzz-smoke` passing (64 cases, seed `0xC0FFEE`)
- Docs gate: `mise run docs-check` passing (consistency, generation, doc tests); LSP RPC smoke passing
- Release readiness: `mise run release-package` and `mise run release-package-check` passing; full-language blockers remain in `docs/RELEASE_READINESS.md`
- Sanitizer gate: AddressSanitizer/UndefinedBehaviorSanitizer passing on SQL lifecycle/quotas/transactions, managed scheduler/ownership/timers, six prior actor scenarios and three TUI scenarios (GC leak reporting excluded).
- Bootstrap gate: Fern-native default checker, exact native/Python diagnostic and 66 workflow parity cases, bounded content cache and native supervision verified on macOS/Linux; ordinary style checks need no Python/Cargo
- Rust migration: `mise run rust-check` passing (1551 Rust checks on macOS with nightly-2026-09-06 (earlier Linux checkpoint: 1518; bounded native actors and relocatable previews), 4 measurement-harness tests, 197 core native programs, 13 newtype programs, 12 namespace programs, 5 labeled-call programs, 27 union programs, 22 entry/access programs, 9 controlled-fault cases, 241 invalid inputs, dual-frontend directory/Result contracts, 192 mutations); expanded frontend remains opt-in; earlier process/stdio and full C/Rust checkpoints passed on Linux arm64
- Backend gate: `mise run rust-cranelift-check` passing on macOS arm64 (1563 Rust checks, 295 independent native-output cases; 2026-09-12), including SQL lifecycle, optional regex captures and exact NaN bit transport
- Rust developer tooling: pinned mise environment and incremental lint-policy checks verified; nextest passes 1511 tests on macOS and 1513 on Linux without skips; Criterion fixtures and all 10 smoke cases pass on both. Actor20-program/16-rejection, managed-runtime sanitizer and moved-preview gates pass on both.

## Canonical Documents

- Project overview: [`README.md`](README.md)
- Build guide: [`BUILD.md`](BUILD.md)
- Design spec: [`DESIGN.md`](DESIGN.md)
- Decision log: [`DECISIONS.md`](DECISIONS.md)
- Coding standards: [`FERN_STYLE.md`](FERN_STYLE.md)
- Compatibility policy: [`docs/COMPATIBILITY_POLICY.md`](docs/COMPATIBILITY_POLICY.md)
- Documentation index: [`docs/README.md`](docs/README.md)

## Completed Foundations

- Gate A: Developer experience and language feel
- Gate B: Reliability and regression resistance
- Gate C: Product surface and stdlib quality baseline
- Gate D: Ecosystem and adoption hardening

## Rust Frontend Evaluation (2026-09-05)

Status: Complete for the bounded prototype; `mise run check`, `mise run rust-check`, and `mise run docs-check` pass on macOS arm64. CI includes Linux/macOS Rust checks. C remains the shipping default (decision 45).

- [x] Build an independent lexer/parser for a clearly bounded Fern subset (11 parser tests).
- [x] Resolve names and types once into a typed intermediate representation (10 checker tests).
- [x] Generate QBE exclusively from typed IR and reuse the C runtime/backend (17 emitter tests and native execution).
- [x] Add check/emit/build/run commands and specification-grounded differential tests (51 Rust tests, 32 native programs, 5 invalid inputs, literal paths).
- [x] Measure clean/incremental frontend builds, check/emit latency, binary sizes, and native execution.
- [x] Record evidence, gaps, and a migration recommendation before expanding scope ([evaluation](docs/RUST_FRONTEND_EVALUATION.md)): continue Rust incrementally; require parity before switching defaults.

## Active Priorities

- [x] Add explicit SQLite close with bounded live connections, stale-handle rejection and rollback/lock release (debug/release/sanitizer groups and exact C/Rust native outputs; Decision113).
- [x] Preserve optional regex capture positions, including absent versus empty groups and later participating groups (seven shared-runtime native cases; Rust source oracle added).
- [x] Promote expired actor timers at every cooperative boundary, preserve deadline/identity wake ordering after timely unmatched or late sends, and retire cached timers without spurious clock faults (2,048 deterministic FernSim timeout dispatches per debug/release/sanitizer mode).
- [x] Preserve NaN sign, payload and signaling bits in QBE machine transport, matching Cranelift without decimal canonicalization (18 independently specified native bit-pattern outputs and serializer regression).

- [x] Showcase native string interpolation in the README greeting; exact `Hello, Fern!` output verified on both C and Rust compilers.

- [x] Refresh the public README around runnable onboarding, accurate feature boundaries and a dedicated Fern logo; greeting and quickstart outputs verified on both compilers, with light/dark presentation checked.

### Development Environment and Rust Review Guidance

- [x] Adopt nightly-2026-09-06 across mise, direct Cargo, CI and developer jobs; verify component identities and drift, 18 negative lint contracts, complete macOS/Linux Rust/native/C/docs/nextest gates and all ten benchmark smoke cases. Bacon check/Clippy and watchexec initial/source-change jobs select nightly (Decision111).

- [x] Replace Justfile with pinned mise tools/tasks and platform checksums; preserve sequential native builds and explicit Zed toolchain scope (Decision106).
- [x] Verify optional nextest, Bacon and watchexec without adding application dependencies (initial Rust1.75 checkpoint; nightly migration above supersedes the project pin).
- [x] Lock all three Python reference-script dependency graphs and reject metadata drift before execution.
- [x] Preserve quoted native compiler flags and pkg-config paths through bounded literal decoding; cover all build helpers and 128 generated argument roundtrips.
- [x] Add the strict incremental Rust lint policy and initial-path fix, with 17 negative/two positive lint contracts, three fixture tests, ten Criterion phase smoke cases and the review-guidance adoption matrix (Decision107).
- [x] Audit native linker-argument and test-frame boundaries under strict lints; preserve Unicode paths, reject malformed/oversized records and validate complete status/payload domains.
- [x] Preserve literal pkg-config library-directory metadata and reject malformed successful records before archive lookup, fallback or link publication.
- [ ] Extend audited strict modules and collect quiet-host comparative phase measurements.
- [x] Verify the combined environment/compiler gates on macOS and freshly provisioned Linux tools: full Rust/native/C/cache/docs gates, 1409/1410 nextest tests without skips, 23 workflow/flag/lock tests, lint contracts and Criterion smoke.

### Rust Migration Completion

- [x] Restore bounded inline match arms with guarded patterns, canonical formatting and explicit nested/caller comma ownership (37 focused parser/formatter checks, ten native migration programs, 512 unchanged seeded parser/formatter cases per compiler; Decision121).
- [x] Preserve consistent tab indentation, eight-column layout and byte-correct spans while rejecting mixed styles; canonical formatting uses spaces (24 focused parser/formatter tests and Clippy; Decision120).
- [x] Restore 14 shipping service aliases, bracket list indexing and infix membership through typed operations (four parser/checker/formatter/REPL tests and four native programs covering full-width values, Float/NaN, source order and deferred bounds-fault cleanup).
- [x] Audit every shipping C builtin name and executable construct against typed Rust lowering (212-name inventory, 48 focused checks, five independent native programs and two C-reference outputs; docs/LANGUAGE_PARITY.md).

- [x] Complete bounded recursive builder contracts and mutual-summary widening without unproved handling credit (90 focused integrations, 76 internal obligation checks; independent empty-subtree cardinality exploit rejected; conservative equations documented in docs/RESULT_HANDLING.md, Decision114).

User authorization: continue through every migration milestone without stopping
for approval between milestones. C remains the default until the parity gates pass.

- [x] Custom algebraic/record types, generic functions, nested patterns and guards (41 checker and 36 emitter tests; native recursive values and guarded matching).
- [x] Modules/imports/visibility and a realistic application spanning multiple files (13 loader tests including visibility bypass regressions, native project execution).
- [ ] Remaining executable language parity: remaining function/numeric/string operations, control flow and complete error handling.
- [x] Replace reference-only Result checks with bounded reachable-path handling, call/alias provenance, complete collection coverage and deferred cleanup (Decision95).
- [x] Prove direct recursive nominal Result handlers, complete List/Map child traversals, exact recursive callbacks and deferred child handling (Decision95 R1–R2).
- [x] Prove complete mutual structural handler groups over actual nominal descendants, including typed List/Map traversal and transparent wrapped children, with independent source-order and unsafe-path probes (Decision95 R3).
- [x] Verify the combined boundary/CLI/Result checkpoint on macOS and Linux: full Rust/native/C/docs gates, 1447/1449 nextest tests without skips, separate doctests and Criterion smoke.
- [ ] Complete recursive builders and wider recursive summary equations without granting unproved handling credit.
- [ ] Standard-library/native ABI compatibility and executable application coverage.
- [ ] Diagnostics, formatting, REPL/LSP, documentation and developer-command parity.
- [x] Preserve common quiet/verbose/color controls, literal forwarded argv, visible failures and missing-command status in the Rust CLI (Decision108A).
- [x] Add bounded source-only lex/parse inspection with byte spans, escaped dumps, no import/type/backend execution and atomic parser/limit failure (Decision108B).
- [x] Open retained HTML documentation only after complete atomic generation, with literal platform argv, visible best-effort failures and consistent byte limits (Decision108C).
- [x] Verify the 1006-test expanded Rust checkpoint, C quality gate and documentation on Linux arm64 with Rust 1.75; expose POSIX test APIs under glibc strict C11 without hiding Darwin extensions.
- [x] Package explicit Rust/native inputs as a relocatable opt-in preview, with deterministic bounded archives, atomic publication, strict verification and missing-helper isolation (Decision110).
- [ ] Linux/macOS verification, fuzz/performance/packaging gates and default migration.

Each checkpoint below records verified scope; this completion list stays open
until all of its acceptance criteria are actually satisfied.

### Rust Migration: Expanded Language and Tools

Status: Expanded checkpoint verified on macOS arm64; the default remains C.

- [x] Custom records/sums, generic specialization, guarded nested matches and modules.
- [x] IEEE Float values, structural tuples/destructuring, pipelines and interpolation.
- [x] Registry-driven runtime signatures and full-width/native-object adapters.
- [x] Formatter preserving syntax/comments and persistent typed-IR interactive evaluation.
- [x] Add non-writing Rust `fmt --check` for CI, with both flag positions, stable status/diagnostics and preserved literal-path/symlink/file metadata (six CLI regressions).
- [x] Module-aware LSP diagnostics over unsaved buffers and UTF-16 edits.
- [x] Replace unsafe argv shell reconstruction with literal process spawning (nine native regressions).
- [x] Return explicit directory listing errors in both frontends and migrate native callers (four ABI, eight native, two binder and two alias checks).
- [x] Preserve complete C lexer state across lookahead and speculative rollback, including bracket nesting, interpolation and pending indentation (three replay regressions).
- [x] Preserve semantic C argument/payload widths and multiline match-arm scope; parser errors terminate (seven parser regressions and native bootstrap parity).
- [x] Preserve signed 64-bit C Int through literals, arithmetic, inferred returns, containers, typed function values and native calls; guard MIN/-1 and inclusive MAX endpoints (16 native programs in debug/release).
- [x] Canonicalize C Unit annotations/empty patterns, unify contextual if branches and expose the existing packed Int Option fallback for checker workflows (annotation, pattern, ABI and branch regressions).
- [x] Verify the complete expanded checkpoint and record test counts (Rust/C/docs/fuzz/native style parity).
- [x] Functions/closures and ten higher-order collection/error operations (287 Rust tests, six new native programs, six negative cases; delayed Result-bearing captures remain restricted).
- [x] Retain originating compiled code for interactive closures, preserve capture/effect order and bound unique retained programs (interactive closure and storage regressions).
- [x] Immutable maps and record updates with semantic keys, persistent aliases and source-order evaluation (six native programs, nine invalid programs and interactive regressions).
- [x] Early returns, postfix conditionals, condition matches, let-else and function-exit deferred cleanup (nine native programs, ten invalid cases and eight interactive control regressions).
- [x] With error handling, List/Map/range iteration and loop control (14 native programs, 14 invalid cases and ten interactive regressions).
- [x] Numeric operators, full-width literal forms, multiline strings/comments/documentation and Unicode identifiers (seven native programs, eight controlled-fault programs, 12 invalid inputs and seven interactive regressions).
- [x] Bound shared runtime String.repeat allocations before multiplication; empty-input fast path (eight native ABI regressions).
- [x] Replace shared runtime list access assertions with defined failures before access (six native ABI regressions; preserves full-width successful values).
- [x] Publish complete bounded File.read text and require buffered write/append completion, with shared REPL text policy (eight source-native cases per frontend and five runtime groups in debug/release/sanitizer builds on macOS/Linux arm64).
- [x] Classify decimal text using checksum-pinned Unicode 16 tables in C/Rust/REPL; preserve size-fault cleanup and charge aggregate interactive work (exhaustive native Unicode oracle and source callback/failure tests).
- [x] Private return inference with bounded recursive constraints, explicit public return signatures and concrete Result-valued entry points (17 checker regressions, bounded type-work checks and native execution).
- [x] Guard Rust list access, repetition and UTF-8 slicing/splitting through deferred cleanup, including native malformed-byte input (21 native programs, 26 shared-runtime UTF-8 cases and interactive regressions).
- [x] Shared exact list and list/tuple rest patterns, bounded coverage and Result discard checks; native length guards and atomic interactive bindings.
- [x] Embedded multiline match/if/for/with/callback suites inside calls, lists and tuples, preserving inline separators and closers (ten native sequence programs, 15 invalid inputs and formatter equivalence).
- [x] Native direct self-tail-call elimination with full-width parameter updates, entry-only scratch storage and preserved cleanup exclusions (million-step native recursion, effects and fault cases).
- [x] Atomic REPL paste entries for complete function clause groups; unfinished input and rejected definitions preserve prior state.
- [x] Typed adjacent function clauses, guards and arrow bodies through shared dispatch, generic inference, modules/formatting and 255-parameter coverage (seven native programs and 11 invalid inputs).
- [x] Optional source-call labels with stable external pattern interfaces, written evaluation order, labeled pipe holes and module/formatter metadata (18 Rust regressions, four native and four atomic invalid programs).
- [x] Require labels for exact Bool and repeated finalized declared scheme types, with bounded work and valid source metadata; migrate source fixtures without changing effect order (ten additional Rust regressions; five native and eight atomic invalid programs).
- [x] Add exact source-label definition/hover across current module overlays and verify labeled grammar/highlights in native/WASM (12 protocol cases and 80/27/27 editor corpus); preserve required labels during member recovery.
- [x] Complete source-label suggestions for closed and EOF-open calls, respecting current modules, supplied arguments, lexical shadowing and exact UTF-16 edits (20 Rust protocol regressions).
- [x] Parse binary/octal/hexadecimal integer and exponent-only Float spellings in the editor, with exact numeric tokens and incremental radix/range edits (93 valid, 33 recovered malformed and 33 incremental native/WASM cases).
- [ ] Complete remaining editor syntax.
- [x] Private parameter inference from all clause patterns and supplied annotations, including delayed tuple-rest arity and nominal payload evidence; public boundaries and ambiguity diagnostics retained (seven native programs, nine invalid inputs and interactive regressions).
- [x] Validate every generic body before specialization using rigid type equality and intrinsic capability requirements; propagate requirements through callbacks/recursive helpers and nominal Map fields (four native programs, 16 invalid programs and interactive regressions).
- [x] Source-based LSP go-to-definition and scoped completion over current overlays, with module visibility, shadowing, UTF-16 edits and bounded output (21 navigation and three source-index regressions; executable protocol smoke).
- [x] Parser-based documentation for individual source files: clause groups, original signatures, types and literal @doc metadata; bounded Markdown/HTML output and atomic CLI writes (15 documentation/CLI regressions).
- [x] Directory documentation with deterministic module navigation and local HTML search; bounded discovery, escaping and atomic source-safe publication (seven new library/CLI regressions and browser search/navigation checks).
- [x] Rust executable documentation tests with checked pattern expectations, original module scope, isolated processes, bounded time/output and explicit library checking (20 Rust regressions and native execution scenarios).
- [x] Regular source-owned unit tests alongside documentation examples, with checked nongeneric entries, failure continuation and test-only process-exit rejection (15 Rust regressions and 15 native cases plus import/directory/doc-mode scenarios).
- [x] Checked inferred documentation signatures from one finalized source-scheme pass per module graph, retaining original headers and protecting all source dependencies (ten Rust regressions and browser search/layout checks).
- [x] Whole private-signature inference and SCC generalization, including delayed shape constraints.
- [x] Core private-signature generalization from patterns and bodies, with callee-first recursive components, rigid annotations and intrinsic requirements (eight native programs, 14 invalid programs and three interactive regressions).
- [x] Delayed shape obligations using later body evidence for fields, updates, tuple-rest and iteration; non-executable probes rejected at publication boundaries (seven native programs, eight invalid programs and two interactive regressions).
- [x] Typed hover and valid-source record/tuple/member completion from final checked source facts, with instantiated uses, inferred requirements, exact doc ownership and separate type/value namespaces (26 new metadata/editor regressions).
- [x] Incomplete member completion from independent concrete receiver evidence in current source, with exact edits and no executable hole IR (21 new Rust regressions including library source identity).
- [x] Preserve global module reference identity when an unrelated local shadows its canonical module name (eight Rust regressions, four native programs and four invalid inputs).
- [x] Transparent scalar/generic type aliases with bounded capture-free expansion, module privacy, original source tooling and native execution (26 new Rust regressions, seven native programs and 12 invalid inputs).
- [x] Distinct generic newtypes with unboxed full-width native representation, explicit projection/patterns, scalar capabilities, Result obligations and source tooling (45 Rust regressions, 13 native programs and 14 invalid cases).
- [x] Independent module type/value declarations and visibility, selected/wildcard reexports, lexical receiver resolution and exact source navigation (19 Rust regressions, 12 native programs and 12 atomic invalid cases).
- [x] Canonical finite unions, contextual member/subset conversion, typed narrowing and full-width native/REPL carriers, preserving generic equality and Result obligations (87 Rust regressions, 27 native programs, 23 atomic invalid cases and unit/main entry checks).
- [ ] Union constructor refinements, variance, implicit joins and lifted capabilities.
- [ ] Remaining function/type syntax and complete native-language audit.
- [x] Track Result duties beyond local references; metadata, partial searches and incomplete branch handling do not acknowledge entire values (Decision95).

### Rust Migration: Collections and Error Values

Status: Complete for the collections/error-value milestone; shipping compiler remains C (decisions 45–46). See [migration progress](docs/RUST_MIGRATION.md).

- [x] Parse recursive List/Option/Result types, list literals, and match expressions (24 isolated parser tests, including all native fixtures).
- [x] Infer constructor/empty-list types and resolve compound values to concrete typed IR.
- [x] Enforce exhaustive matches and consistent payload/branch types.
- [x] Propagate Result errors with postfix `?`, preserving the enclosing error type.
- [x] Lower immutable lists and full-width Option/Result values through the existing runtime.
- [x] Verify native collections, errors, nested payloads, scoped patterns, and invalid programs (31 native programs, 22 negative cases; includes retained pointers across 1,500 allocations).
- [x] Update the supported-feature guide and pass Rust/C/documentation gates (94 Rust tests, 542 C tests, clippy/format, exact native output, docs).

### Priority 1: Repository Hygiene and Release Flow

Status: Complete for the tracked hygiene tasks

- [x] Fix release staging validation mismatch (`release-package-check` now stages and validates `dist/staging`)
- [x] Keep generated outputs out of source control by default (release artifacts, benchmark binaries)
- [x] Keep documentation cross-linked and remove stale process instructions
- [x] Add a lightweight docs consistency check to CI (link + key status marker validation)

Exit criteria:
- Working tree does not accumulate tracked generated binaries during normal benchmark/release flows.
- Every top-level process document links to canonical docs and active roadmap.

### Priority 2: Milestone 8 Follow-Through (Actor Runtime Semantics)

Status: Active

- [x] Enforce restart budgets at time zero, single replacement lineage, acyclic ownership, invalid PID rejection, and stopped-sibling semantics (six runtime scenarios, 1,536 seeded crash steps)
- [x] Preserve runtime `send` Result values through native codegen (success and invalid-PID execution regressions)
- [x] Stop owned descendants before fallible exit notifications, reject restarts under dead owners, and prepare name/monitor storage before publishing actors (ten subtree/failure groups plus all six prior scenarios in debug/release/sanitizers on macOS/Linux arm64).
- [ ] Close remaining supervision/runtime behavior gaps not yet modeled end-to-end
- [ ] Expand deterministic FernSim scenarios for supervision trees and failure policies
- [x] Add stronger actor runtime invariants to regression suites (seeded native-runtime scenarios)

Exit criteria:
- Actor runtime semantics are deterministic, regression-covered, and documented as compatibility commitments.

### Priority 3: Milestone 9 Bootstrapping

Status: Complete for the native quality-checker bootstrap workflow

- [x] Compare exact diagnostics, severity, messages, and exits on pinned failing fixtures and all `src`/`lib` sources (strict, lenient, summary, nested paths)
- [x] Port the Fern checker to immutable returned state and verify exact diagnostics, file counts and exits under both C and Rust frontends, including continuation after a failed build.
- [x] Capture bounded literal processes through both native frontends with full-width limits, distinct normal/error results and owned cleanup (31 native scenarios and five atomic rejections per frontend; 21 runtime groups in debug/release/sanitizer builds).
- [x] Add fallible exact stderr output with thread-local SIGPIPE preservation (nine native groups in debug/release/sanitizer builds; six source-native cases and two atomic rejections per frontend).
- [x] Accept interpolated strings as indented C-parser body expressions (three AST regressions), including final branches followed by else.
- [x] Run the native build/test/example/Git workflow through literal bounded argv, preserve stderr CLI failures and compare 47 scenarios under both frontends.
- [x] Reach pinned diagnostic/workflow parity for `scripts/check_style.py` in `scripts/check_style.fn`, including Unicode numeric-path prefix classification (66 reference-first workflow cases under both frontends).
- [x] Add parity assertions to CI (`mise run style-parity` as a required gate)
- [x] Make the Fern-native checker the default through a content-validated C-bootstrap launcher; verify cold/warm caches, source and external inputs, concurrency, failure, ownership and independent Python parity on macOS/Linux (Decision93).

Exit criteria:
- Style checks can run without Python for normal developer workflows.

### Priority 4: Milestone 10 TUI Completion

Status: Complete for the tracked TUI scope

- [x] Finish prompt line editing and richer terminal cursor controls
- [x] Implement tree/log modules for structured CLI UX
- [x] Add canonical TUI examples with deterministic test coverage

Validation: 13 native and PTY tests cover input/password editing, Unicode, EOF/cancellation, terminal restoration, cursor controls, immutable trees, log escaping, and the canonical Fern example. Other prompt variants retain line-based input.

Exit criteria:
- TUI surface is complete enough to support first-party tooling UX needs.

### Priority 5: Milestone 11 Polish and Optimization

Status: Active

- [ ] Reduce binary size and startup variance further while preserving ergonomics
- [x] Tighten release checklist and pre-1.0 readiness criteria
- [x] Expand user-facing docs (language guide/tutorial) for adoption

Exit criteria:
- Pre-1.0 release checklist is explicit, measurable, and continuously validated.

## User Workflow Closure (2026-09-05)

- [x] Validate real release archive names and reject unsafe version/platform metadata.
- [x] Install the runtime with the compiler; support local `PREFIX` and staging `DESTDIR`.
- [x] Resolve relocated/PATH/symlinked compiler bundles and quote toolchain paths.
- [x] Isolate `fern run` artifacts; never overwrite basename-derived files in `/tmp`.
- [x] Accept build flags before or after the source file.
- [x] Reject unsupported autonomous spawn/receive execution with actionable diagnostics.
- [x] Preserve quoted, escaped, Unicode, and long string literals through native codegen.
- [x] Print declared String/Bool function results and conditional/comparison expressions correctly.
- [x] Execute canonical examples and four tutorial programs with exact output assertions.
- [x] Replace the stale HTTP placeholder example with deterministic client error handling.

## Remaining Language Completion Requirements

The design document is broader than the executable implementation. See
[release readiness](docs/RELEASE_READINESS.md) and [actor contracts](docs/ACTOR_RUNTIME.md).
Do not interpret the historical Gate A–D labels as language completion.

- [x] Reproducible Tree-sitter aliases/newtypes/function-clause checkpoint: native/WASM accepted/recovery/incremental corpus, executable editor queries, bounded scanner and pinned generated artifacts (Decision 84).
- [x] Extend the pinned editor grammar to finite unions, function-type precedence and typed narrowing; verify 38 accepted, 12 recovery and 13 incremental sources with native/WASM structural equivalence.
- [x] Verify control/collection editor syntax against 69 Rust-checked programs, 20 malformed inputs and 22 native/WASM edits; isolate parser caches and enforce the exact 1 MiB scanner boundary.
- [x] Recover the following declaration after malformed inline for/with headers, preserving the original sources and genuine errors (85 valid, all 33 malformed recovered, 30 native/WASM edits).
- [x] Verify Zed grammar registration, pinned Preview2 component packaging, reproducible staged archives and actual isolated Rust LSP startup through discovery and explicit paths.
- [x] Repin the locally staged Zed package to the verified label/recovery grammar, test exact label captures and reproducible archives, and rerun both actual-editor LSP discovery/override smokes.
- [x] Repin the locally verified Zed package to the numeric-literal grammar and rerun package/actual-editor checks.
- [x] Format explicit source directories and check all dirty paths without writes; validate all inputs and stage all replacements before publication (Decision99).
- [x] Expose canonical full-document formatting through Rust LSP over current unsaved buffers, with UTF-16 edits, no server-side publication and explicit syntax/parameter errors (Decision100).
- [ ] Complete Tree-sitter parity for remaining Rust syntax and publish a fetchable matching grammar revision when release is authorized.
- [x] Execute bounded Rust native actor functions with typed mailboxes, selective receive/timeouts, explicit continuation frames, invocation-owned quotas and fault cleanup (Decision105A; native20-program and independent lifecycle/sanitizer gates).
- [ ] Extend actor execution to generalized suspension, typed ancestor escalation/descendant subtree reconstruction, REPL and FernSim parity.
- [x] Immutable native JSON parser/accessors/stringifier with exact numbers, Unicode validation and bounded resources (14,309 API checks, 24 budget checks and 6,000 numeric oracle cases in debug/release/sanitizer builds).
- [x] Migrate Rust native JSON to opaque values/errors, immutable builders and bounded lossless collection adapters (ten native programs, twelve semantic rejections, eight Rust integration tests and 248 native builder checks per build).
- [x] Evaluate dynamic JSON in the Rust REPL with exact native semantics, independent cleanup budgets and bounded shared storage (21 new Rust regressions and 12,000 numeric/formatting oracles).
- [x] Implement concrete typed JSON codecs: explicit derive(Json), static targets, native/REPL shared plans, strict fields and bounded error paths (Decision98 J4; 4 native programs, 14 atomic invalids and runtime debug/release/sanitizer boundaries).
- [x] Verify record derivations and static JSON targets in native/WASM editor grammar (98 valid sources, 34 malformed cases recovered, 36 incremental edits).
- [x] Repin the Zed package to the typed JSON grammar and verify reproducible archives, hostile-package rejection and both actual-editor startup modes with derived-record codec source.
- [x] Support regular recursive JSON records through finite indexed plans, with a charged finite-value proof and native/REPL depth boundaries (Decision101 J5a).
- [x] Derive transparent JSON codecs for newtypes, preserving full-width payloads without wrapper allocations and distinguishing nullable fields from optional fields (Decision101 J5b).
- [x] Verify newtype derivation syntax, the 32-trait limit, recovery, queries and incremental edits in native/WASM editor grammar (102 valid, 38 malformed and 41 edits).
- [x] Repin Zed to the newtype grammar commit 5b02ec6; verify reproducible packages, 102/38/41 grammar cases and actual-editor LSP startup in override/discovery modes.
- [x] Extend typed JSON with conditional generic codec requirements, exact specialization, private template boundaries and phantom-field semantics (Decision101 J5c; 13 native programs, 22 atomic invalids, native/REPL parity).
- [x] Implement explicitly derived tagged JSON sums, finite constructor-choice proof and shared native/REPL resource accounting (Decision103 J6a).
- [x] Implement conservative disjoint JSON unions, allocation-free member selection and whole-union conditional requirements (Decision103 J6b).
- [x] Accept inline union-bearing JSON decoder targets with bounded linear recognition, exact type identities and native/interactive parity; preserve ordinary parsing/formatting.
- [x] Verify the library-path/inline-codec checkpoint on macOS and Linux: full Rust/native/C/docs gates, 1470/1472 nextest tests without skips, separate doctests and all ten Criterion smoke phases.
- [ ] Complete deeper union discrimination and general/custom Json traits (J6c–J7).
- [x] Reassess QBE/Cranelift using current primary sources and an independent native AOT experiment; correct the original QBE rationale (Decision109).
- [ ] Trial a supported Cranelift backend through shared lowering and complete native/ABI/debug/performance gates before a default decision.
- [x] Extract shared typed machine lowering and replace handwritten QBE helper text with structured Rust builders; preserve existing QBE boundary tests (Decision112).
- [x] Add canonical native signatures, fixed Float runtime boundaries and explicit Cranelift build/run selection; selected builds emit objects without invoking QBE or an assembler.
- [x] Preserve the complete default Rust/QBE gate on macOS/Linux ARM64 after shared lowering (1,548/1,551 Cargo tests, native and packaging/workflow oracles, 192 fuzz mutations).
- [x] Revalidate the full C quality and documentation gates on macOS/Linux ARM64; add Cranelift native acceptance to the Linux/macOS CI matrix with bounded test artifact profiles.
- [x] Pass the complete Cranelift feature gate on macOS/Linux ARM64: 293 independent native output oracles per platform, 1,559/1,562 Cargo tests, feature Clippy, and mixed-argument ABI, relocation, phi and forced-GC regressions. Source debugger, controlled performance, x86-64 and default-promotion gates remain separate.
- [x] Reserve an ABI-permitted Apple arm64 QBE scratch register, verify swaps/calls/spills and independent native outputs, and preserve generic Linux assembly (Decision104).
- [x] Remove newly written executable races from native capture tests; preserve timeout/stream/descendant coverage and add 160 concurrent per-run output/status assertions.
- [x] Retain direct-child ownership through native unit/doc test cleanup and validate the framed safe-Rust adapter (Decision102).
- [ ] Verify default-command migration to Rust, retaining C as an explicit bootstrap/reference executable and documenting its legacy JSON source contract (Decision96).
- [ ] Implement HTTP serving and the broader SQL query/resource APIs described in the design.
- [ ] Complete function clauses/pattern parameters, labeled calls, aliases/newtypes/unions, traits/constraints and full private signature inference through native execution.
- [ ] Complete Sets and the specified standard modules, including data formats, testing/utilities, IO/system, cryptography and compression.
- [ ] Audit all specified syntax and stdlib calls for complete typecheck-to-native behavior; reject unsupported execution paths.
- [ ] Complete ownership analysis and the planned WASM backend.

## Next Session Start Here

For the active Rust migration, finish bounded process/bootstrap workflows and remaining editor grammar parity, then the remaining
specified syntax and native/stdlib parity.
Control flow, closures, maps, function clauses and generic-body validation have
verified checkpoints. Preserve the concrete type/ABI and native-output gates in
[migration progress](docs/RUST_MIGRATION.md).

1. Extend the bounded native actor checkpoint to generalized suspension, typed supervision and REPL/FernSim parity; preserve the verified mailbox and lifecycle contracts.
2. Preserve the verified native checker default and its C-bootstrap/reference parity while completing the Rust command migration.
3. Close remaining spec-to-execution gaps using native-output tests, starting with JSON and server/data APIs.
