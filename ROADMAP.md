# Fern Roadmap

Last updated: 2026-09-15

This file is the only active roadmap. Historical context is in [`docs/HISTORY.md`](docs/HISTORY.md).

## Current Status Snapshot

### Morrow rename (2026-09-15, Decision155)

- [x] Rename all 12 workspace packages, component paths, CLI, runtime symbols and Cargo tooling. macOS ARM64 `cargo xtask check` passes: 2,249 Rust tests, 316 native-output fixtures, 20 examples, 63 dynamic programs, 295 atomic rejections and 64+192+231 fuzz cases; dependency versions and persisted cluster identities remain unchanged.
- [x] Rename all 455 source files to `.mr` and update discovery, module imports, formatter, LSP requests and test/documentation fixtures. An independent discovery test fails before and passes after migration. The full macOS ARM64 `cargo xtask check` passes; all three standalone benchmark workspaces also pass tests, formatting and Clippy.
- [ ] Update active documentation, public introductions, canonical URLs and `MORROW_STYLE.md`; preserve historical evidence and verify the full repository gate.
- [ ] Replace old logo usage with a Morrow text mark and update browser/preview branding; verify web output and the full repository gate.
- [ ] Transfer and rename the GitHub repository to `morrow-lang/morrow`, update metadata and origin, and create `morrow-lang/morrow-lang.github.io` for the docs site.
- Owner follow-ups: register both domains, configure the `.dev` → `.org` redirect, complete the EUIPO/DPMA classes 9/42 check, and supply a new logo.

- Fern-owned implementation: Rust throughout the compiler, native runtime, language server, supervisor and repository tooling. The old C/QBE/bootstrap setup and Tree-sitter integration are removed. The recorded migration passed complete debug quality gates, selected optimized runtime/ABI checks and actual release archive/installation workflows on macOS/Linux ARM64. [Migration scope and evidence](docs/RUST_WORKSPACE.md); new actor/web work has separate validation below.
- Active product direction: supervised native actors and a reactive Fern WebAssembly client, connected through typed WebSocket messages (Decision124). Actor payload heaps and copied messages, explicit compiler roots, a separate aggregate WASM backend, typed Fern browser model/update/view and compiled native room actors are implemented. Optional durable room checkpoints pass native restart and real WebSocket acceptance. General framework packaging and scalable actor execution remain open; [preview scope](docs/WEB_PREVIEW.md) and Decision125 distinguish them from the completed Rust migration.
- System observability: an authenticated, read-only [dashboard](docs/ADMIN_DASHBOARD.md) and JSON endpoint expose host/build information, current/peak process memory, admission use and bounded per-worker observations without queuing behind native callbacks (Decision131). The preview shares its application access policy; separate operator roles, CPU utilization and individual actor/GC breakdowns remain open.

### Full-stack Actor and Browser Milestones

- [x] Add current and peak process RSS to the authenticated dashboard and JSON snapshots. Verify native macOS/Linux samplers with checked unit/parser cases and an isolated touched 32 MiB allocation; inspect the running macOS demo. The complete macOS `cargo xtask check` passes 1,936 Rust tests across 254 suites, 305 native fixtures, 19 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192 fuzz cases, plus formatting, notices and Clippy. Memory readings describe the whole server process; unavailable observations remain explicit.

The [architecture and acceptance plan](docs/FULL_STACK_ARCHITECTURE.md) defines
ownership, scheduling, browser ABI and protocol contracts. The completed items
below describe bounded foundations, not completion of the larger acceptance plan.

- [x] Add an authenticated read-only system dashboard and schema-versioned JSON snapshots with bounded worker telemetry, last-observed occupancy, admission usage and host/build information (Decision131). Pass 12 optimized owner tests, nine real HTTP/WebSocket tests, desktop/small-phone inspection and the complete browser suite. Final macOS `cargo xtask check` passes 1,933 Rust tests across 254 suites, 305 native fixtures, 19 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192 fuzz cases, plus formatting, notices and Clippy.

- [x] Adopt automatic memory without mandatory application borrow checking, actor-owned tracing heaps, a separate browser ABI and a Rust-authored first-party web/UI framework (Decision124).
- [x] Branch a separate WASM emitter from checked semantic IR, preserve i64 integers, execute scalar exports and reject unsupported types/capabilities through imported code before publishing output.
- [x] Add actor-owned payload heaps and bounded message/capture graph copying, with explicit compiler root frames and scoped runtime roots. Conservative native stack/register and heap-word scanning remain enabled.
- [x] Add a bounded precise linear-memory String heap to the WASM backend with compiler shadow roots and independent generated-module execution tests.
- [x] Build a Rust browser host with keyed accessible DOM updates, local filtering, focus preservation, bounded drafts/snapshots and explicit listener/socket cleanup. Compile and execute the Fern checklist policy in the real browser.
- [x] Add Rust HTTP/WebSocket serving and a typed command/snapshot protocol with session authentication, exact Origin/CSRF checks, admission and queue bounds, revision conflicts, bounded deduplication, revocation and explicit reconnect/reset outcomes.
- [x] Package the preview with embedded assets, generated bindings and a Rust offline service worker. Exercise two real clients, cached offline reload, drafts, reconnect and logout against the static ARM64 Linux server running unprivileged in an empty chroot. Validate x86-64 static ELF structure; x86-64 execution remains open.
- [x] Pass final macOS real-browser acceptance including compiled Fern policy, keyed focus, cold service-worker restart, offline draft/filter recovery, reconnect/revocation, a 320-pixel mobile layout and rejection of a tampered HTTP-200 cache update while retaining the prior offline application. Inspect desktop and mobile screenshots.
- [x] Revalidate the new work with macOS ARM64 `cargo xtask check` and equivalent Linux ARM64 coverage across a workspace check and resumed acceptance tail after storage recovery: formatting, Clippy, notices, workspace tests, 305 native fixtures, 18 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192 fuzz cases. Separately pass 17 WASM, 17 native ABI and four compiler-root cases. Add the static x86-64 browser CI job; its GitHub execution remains unverified.
- [x] Pass the 2026-09-13 integrated macOS ARM64 `cargo xtask check`: formatting, complete notices, workspace Clippy, 1,869 standard Rust tests, 305 native-output fixtures, 18 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192 fuzz cases. Pass the real browser i64-handle/offline/mobile/cache-integrity gate and link native application test executables statically for ARM64/x86-64 Linux. New Linux execution evidence remains separate from earlier preview acceptance.
- [x] Reuse actor slots under immutable nonwrapping generations. Pass 66,536 sequential spawn/finish transitions with precise control collection and bounded memory; preserve stale PID rejection, foreign-heap identity roots, supervision lineage and eventual dead-control reclamation (Decision129).
- [x] Suspend eligible recursive Unit-tail helper paths through copied, rooted callback frames. Independent native tests prove sibling progress, receive-arm/timeout and aliased-entry coverage, mutual recursion with collection at every handoff, and unchanged ordinary/no-actor CLI calls (Decision129).
- [x] Add seeded application simulation using the production protocol/client state machine and compiled native Fern actors, with per-session virtual time, bounded transport faults, actual durable reopen, an independent state oracle and mandatory healthy convergence. Versioned JSON reports replay exactly; forced corruption fails the checker (Decision130).
- [x] Add deterministic native callback-ABI scenarios for receive deadline ordering, yielding siblings, supervision, identity churn, clock errors and final managed-heap reclamation. A precise-collection test protects Session construction before host-root publication; simulation controls remain opt-in (Decision130).
- [x] Add a visible local draft preview and UTF-8 budget computed by Fern WASM, plus scoped pending save feedback, informed by the pinned LiveView source study. Independent native and Wasmi traces preserve the existing 100-task/two-held-view bound; final real-browser acceptance is recorded separately.
- [x] Fix durable recovery after deleting the last task, preserve valid empty decoded collection storage, and copy native map entries using the compiler’s untagged pair layout. Root partial typed JSON values across recursive allocation. Independent checkpoint, compiled Fern UTF-8/full-width map and forced precise-collection regressions pass.
- [x] Pass the final 2026-09-13 resilience macOS ARM64 `cargo xtask check`: formatting, notices, workspace Clippy, 1,924 Rust tests across 254 suites, 305 native fixtures, 19 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192 fuzz cases. Pass final real-browser offline/feedback/mobile/cache-integrity acceptance, exact application/actor simulation replay and static ARM64/x86-64 Linux ELF checks. [Measured scenarios and artifact hashes](docs/DETERMINISTIC_SIMULATION.md).
- [x] Execute the resilience checkpoint `3be4025` on actual ARM64 Linux: 138 optimized runtime, checkpoint/clock, simulator and backend tests; byte-for-byte cross-platform replay of the macOS application/actor reports; full browser acceptance against the checksum-matched static ARM64 server, with desktop/mobile inspection. Keep x86-64 static validation distinct from execution.
- [x] Reproduce and fix CI fixture launch-state failures without weakening their oracles: explicit opener failure configuration and visible, painted screenshot readiness under the existing deadline. Pass the follow-up local gate with 1,927 Rust tests and all native/example/compatibility/fuzz checks; real-browser readiness starts from a hidden page. GitHub CI results are tracked separately from local acceptance.
- [x] Build and execute the static x86-64 Linux server in GitHub Actions at `a5f13e2`; pass the complete real-Chrome browser suite, including offline reload, Fern feedback, reconnect, mobile layout and cache-integrity rejection. [Job evidence](https://github.com/niklas-heer/fern/actions/runs/34773888739/job/103768321541).
- [x] Reproduce CI launcher descriptor leakage with an explicitly inherited non-CLOEXEC handle, including a handle above a lowered file limit. Fence inherited descriptors before the supervisor's `posix_spawn`, retaining the existing status, stream, descendant and cleanup oracles. All 27 optimized supervisor tests and Clippy pass on macOS ARM64 and actual Linux ARM64.
- [x] Pass the final descriptor-fix macOS ARM64 `cargo xtask check`: formatting, notices, workspace Clippy, 1,928 Rust tests across 254 suites, 305 native fixtures, 19 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192 fuzz cases.
- [ ] Complete precise native root/layout coverage and general suspended-continuation lifetime checks. Charge copying and collection to measured work budgets.
- [ ] Extend resumable work-budget safepoints to arbitrary helper calls and collection loops; add an external-event scheduler and bounded blocking-service workers. Existing typed supervision and Unit-tail progress do not establish general preemption.
- [x] Extend WASM to records, tagged sums, Option/Result, lists and tuples with precise child tracing, managed i64/BigInt host handles and bounded UTF-8 transfer. Independent nested/full-width/Unicode/GC oracles and complete shared model/update/view traces pass. Maps, closures and indirect calls were added subsequently; see [current target support](docs/WASM_LANGUAGE.md). Native capabilities and WasmGC evaluation remain separate work.
- [x] Move the complete checklist browser model/update/view and server domain model into typed Fern. Link native Fern room actors into the Rust gateway with rooted thread-confined sessions, nonblocking polling and typed String reply ports. Real browser and WebSocket acceptance passes; generic rendering stays Rust. Shared wire-codec generation and application-independent packaging remain open.
- [x] Add actual compiled actor supervision with bounded restarts, fresh initializer heaps, cleanup, stale-PID rejection and unrelated actor progress between restart attempts. Add recoverable native embedding exports without process startup or an interpreter.
- [x] Pass independent native embedding tests for persistent room state, isolated rooms, full-width values, recoverable faults and 5,000 requests with precise host-root collection.
- [x] Add optional single-writer atomic room checkpoints before acknowledgement; recover tasks under fresh incarnations and command namespaces. Native reopen/lock/corruption tests and a real WebSocket server restart pass. This is room-state durability, not durable exactly-once external effects.
- [x] Pin rooms to configurable independent workers while sharing global ingress, room, namespace and connection limits. Prove other-worker and authentication progress while one domain callback is blocked, reject revoked queued commands, and preserve cross-worker reconnect at capacity. Share a durable writer with stale-owner rejection and independent concurrent-room recovery tests (Decision128).
- [x] Pass the integrated 2026-09-13 worker/lifecycle macOS ARM64 `cargo xtask check`: formatting, dependency notices, workspace Clippy, 1,890 Rust tests, 305 native fixtures, 18 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192 fuzz cases. Includes canceled login/join/disconnect cleanup, partial worker startup and active ingress-permit ownership.
- [x] Rebuild final macOS/ARM64-musl/x86-64-musl web artifacts at `f80f55a`. Pass macOS browser acceptance and 98 focused optimized tests on actual ARM64 Linux, including six native compiler-tail oracles, plus the full browser suite against the checksum-matched static ARM64 server. Record [artifact sizes and exact scope](docs/WEB_APPLICATION_ACCEPTANCE.md); x86-64 remains statically validated without execution.
- [ ] Pass the full architecture acceptance cases with actual Fern domain actors, actor failure/reset and independent aggregate ABI/GC oracles. Room sharding establishes independent-worker progress; arbitrary-helper scheduler fairness and live actor migration remain open.
- [x] Connect fixed room owners across configured servers with mutual TLS, certificate-bound identities, bounded forwarding streams, fresh-namespace uncertainty and checkpoint placement guards. Pass independent routing/lease simulation, hostile framing/authentication tests and a three-process 10,024-durable-mutation campaign across 32 clients; preserve state through partitions and owner restart. See [cluster contract and measurements](docs/CLUSTER.md).
- [x] Establish the initial Decision144 JSON baseline using primary Erlang, Phoenix, WebSocket, WebTransport and codec sources and 46 measured message fixtures with isolated CBOR/protobuf adapters and malformed-input oracles. Binary dependencies stayed outside production during that initial experiment; Decision145 below supersedes the default after browser and application measurements.
- [x] Extend the isolated codec comparison to real Chromium/WASM string and byte-array boundaries, with independent wire/value/malformed-input checks, 138 raw fixture rows and recorded artifact sizes. Run 7,650 loopback WebSocket mutations through the actual Hub, compiled Fern actor and optional durable checkpoint path. All exact transition and durable reopen checks pass. Binary messages reduce traffic and browser decode work; the larger application cases show essentially unchanged latency. Stage wall-time fractions do not establish CPU shares. See [measurements](docs/NETWORK_PROTOCOL.md).
- [x] Select protobuf for live browser and peer messages after those measurements (Decision145). Publish explicit application and peer schemas, retaining legacy browser JSON negotiation and separate HTTP/offline/checkpoint formats. This is the architectural selection; the integrated migration gate below remains separate.
- [x] Complete protobuf migration acceptance: both explicitly selected browser codecs, binary JS/WASM boundaries, mismatch and hostile-input rejection, cross-codec state convergence, real-browser offline/reconnect behavior, authenticated peer framing, partition/lost-response/restart stress, deterministic simulation and the complete repository gate. Do not treat the earlier JSON cluster stress measurements as a protobuf throughput result. Verified on macOS ARM64: 2,176 Rust tests, 311 native-output fixtures, 20 examples, 63 dynamic compatibility programs, 295 atomic rejections, 64+192+231 fuzz cases, formatting, notices and Clippy; real Edge acceptance includes two clients, offline/reconnect, session revocation and tampered-asset rejection. The separate mixed-gateway test preserves exact outcomes and state across JSON and protobuf clients.
- [x] Release checkpoint locks at writer teardown and on initialization failure even when a concurrent child retains a duplicated descriptor. Two independent regressions fail with the old lock lifetime and pass with explicit owned-guard release on macOS and Linux; active-writer exclusion and placement checks remain intact. The follow-up macOS `cargo xtask check` passes 2,178 Rust tests and the complete native/compatibility/fuzz gates, and the rebuilt server passes real-browser acceptance.
- [x] Compare the initial Fern baseline with optimized Rust and Bun 1.4.2/TypeScript 6.0.2 using runtime-input scalar and immutable-model workloads, independent mathematical oracles, compiler mutation diagnostics and positive controls. Preserve 174 performance samples, full-width Number/BigInt observations, process RSS, source build/check timings, binary sizes and first-launch outliers. That baseline uses little memory and checks small sources quickly, but immutable model updates are substantially slower; its generated native code selects Cranelift `opt_level=none`. See the [historical experiment report](benchmarks/language-comparison/README.md); these tests do not establish developer productivity or whole-language superiority.
- [x] Profile immutable callbacks and replace repeated GC frame tree updates with reusable storage and direct range scanning (Decision146). Preserve exact payloads, old versions, heap isolation and fault cleanup with independent native oracles, 8,192 seeded lifetime operations and a failing-before/passing-after allocation regression. The unchanged model is 2.73–2.94× faster with similar RSS. Full macOS gate: 2,183 Rust tests, 311 native fixtures, 20 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192+231 fuzz cases pass; see the [paired measurements](benchmarks/language-comparison/IMMUTABLE.md).
- [x] Enable optimized native emission, prove bounded direct list traversal/builders, and inline small statically known list callbacks while retaining dynamic/control/actor fallbacks (Decision147). Preserve 342 staged timing samples: unchanged immutable updates improve another 2.98–3.71×, scalar performance is unchanged, and small-source builds grow about 4%. Full macOS gate passes 2,193 Rust tests, 311 native fixtures, 20 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192+231 fuzz cases, including precise-GC, exact-payload, fault/cleanup, historical-alias and fallback oracles. See [native optimization evidence](benchmarks/language-comparison/NATIVE_OPTIMIZATION.md).
- [x] Compare the optimized native implementation with Elixir 1.20.4 on OTP 29.0.6/JIT and optimized Rust. Preserve 224 whole-process and 40 in-VM samples, 1,744 independent result checks, initial immutable aliases, tuple/struct alternatives, startup, RSS and pinned artifact hashes. Separate startup-amortized work from BEAM compute timing; this establishes no actor, scheduler or distributed-system performance parity. See the [BEAM experiment](benchmarks/language-comparison/BEAM.md).
- [x] Expose safe constant integer division/remainder and retain eligible all-Int self-tail parameters in SSA (Decision148). Preserve 113,508 independent wide-arithmetic outputs, 357 recurrence/permutation cases, operand/fault/cleanup/root behavior and 246 staged timing samples. The final scalar workload improves 76.60→66.57 ms against Rust's 57.35 ms; immutable-model timings are essentially unchanged. Retain the slower constant-only experiment and verify both changes are needed with an SSA-only control. Full macOS gate passes 2,202 Rust tests, 311 native fixtures, 20 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192+231 fuzz cases. See [arithmetic evidence](benchmarks/language-comparison/ARITHMETIC.md).
- [ ] Continue profiling broader inlining, immutable collection representation/allocation, native actor execution, the native JSON bridge and checkpoint work independently. Preserve exact output, rooting/GC and fault oracles when improving measured bottlenecks.
- [ ] Prove sustained multi-worker fairness, lifecycle churn and performance beyond the bounded cluster acceptance workload. Add transactional external-effect recovery, dynamic membership and replicated ownership/failover as separate gates.

### Rust Workspace Completion

- [x] Organize the Cargo workspace under `crates/` with compiler, core runtime, startup archive, shared JSON engine and supervisor boundaries; keep repository automation in `xtask`.
- [x] Port full-width values, strings/lists, Unicode16 decimal classification, JSON/codecs, IO, HTTP/SQL/regex, terminal widgets and actors to Rust.
- [x] Replace Boehm with a Rust-owned nonmoving collector. Root/interior-pointer, cycle, pressure and finalized-value checks pass, including a pointer retained only by Cranelift-generated code across collection.
- [x] Root all managed strings held only by a Rust Vec before allocating copies. The regression reproduced corruption before the fix and passes debug/release with forced collection.
- [x] Use Cranelift for all native compilation, including source tests and doctests. Independent output fixtures and optimized ABI/GC tests pass on both ARM64 platforms.
- [x] Replace native process supervision with Rust retained-identity cleanup and a shared bounded parent protocol. Lifecycle, cancellation, descriptor, descendant and terminal tests pass.
- [x] Move building, checking, fuzzing, measurement, packaging, installation and uninstall to Rust tooling. Retire C/Python bootstrap, style and maintenance implementations; preserve executable compatibility assertions in Rust.
- [x] Remove Tree-sitter and its Zed grammar package; keep the Rust LSP. Use static accessible generated documentation with browser Find, removing the authored JavaScript filter.
- [x] Prefer native Rust dependencies while allowing Rust wrappers around third-party native libraries, as explicitly authorized. Preserve SQLite through rusqlite and certificate-verifying HTTP through ureq/rustls; retain complete notices.
- [x] Pass final debug quality gates: 1,752/1,754 standard workspace tests plus five custom IO/PTY cases per macOS/Linux host; 305 independent native fixtures, 18 examples, 63 dynamic applications, 295 atomic rejections, unit-test continuation and 64+192 fuzz cases on each.
- [x] Pass targeted optimized runtime/JSON/supervisor checks (92 standard tests plus five custom protocols per host) and 12 separate object/ABI/GC tests. This does not claim the entire compiler integration suite was run optimized.
- [x] Preserve 26 Rust distribution contracts and 18 negative/two positive lint policy probes. Both platforms pass three compiler-phase fixture tests and ten benchmark smoke phases.
- [x] Complete refreshed release artifact, relocation/install/source-test/uninstall and performance records for both ARM64 platforms. Archive and performance reports identify matching component hashes; no x86-64 execution, complete optimized compiler suite or 1.0 release is implied.
- [x] Refresh the public README around Fern's purpose, Rust implementation policy, verified platform scope and preview limits. Execute the greeting, quickstart run/build and HTML documentation command; verify local links and contributor guidance.

### Previous Compiler-Default Acceptance

- Rust compiler migration: complete on macOS arm64 and Linux arm64. Build, install and release workflows select Rust as `fern`; `fern-c` remains the explicit C bootstrap/reference. [Acceptance evidence](docs/history/RUST_DEFAULT_MIGRATION.md).
- Compiler gates: Rust/QBE passes 1,621/1,624 Cargo tests on macOS/Linux; Cranelift passes 1,633/1,636. Both platforms pass 305 independent backend native-output cases and ten migration programs.
- Quality gate: full clean quality gates pass, including 574 C tests, 18 examples, runtime/native oracles, strict style and 66 workflow parity cases; documentation and real-server LSP checks pass on both platforms.
- Fuzz gate: unchanged 64-case `0xC0FFEE` smoke passes for both compilers on both platforms; the 512-case compatibility stretch passes for Rust and C on macOS. Rust's separate 192-mutation corpus passes on both platforms.
- Bootstrap and developer tools: full native style-launcher checks, 18 negative lint contracts and all ten compiler-phase benchmark smoke cases pass on both platforms. Opener and retry-limit fixtures now have deterministic execution/clock inputs.
- Perf gate: combined release builds take 44.21 s on macOS and 44.01 s on Linux. Rust compiler sizes are 3,823,744/3,806,232 bytes and startup p95 is 6.62/0.41 ms; C independently retains its 1,500,000-byte ceiling. All enforced budgets pass.
- Docs gate: documentation consistency, eight snippet-runner regressions and the executable stdlib example pass on both platforms.
- Distribution: nine installation checks and ten checks of each actual relocated release archive pass. Exact compiler/helper/runtime/license inputs and matching source-tree hashes are recorded in the acceptance report.
- Release readiness: QBE remains the default backend; the C runtime, native supervisor and editor parser retain their existing implementations. Generalized actors, remaining language/stdlib/editor work, backend promotion and a 1.0 release have separate exit criteria in [release readiness](docs/RELEASE_READINESS.md).

## Canonical Documents

- Project overview: [`README.md`](README.md)
- Build guide: [`BUILD.md`](BUILD.md)
- Design spec: [`DESIGN.md`](DESIGN.md)
- Decision log: [`DECISIONS.md`](DECISIONS.md)
- Coding standards: [`FERN_STYLE.md`](FERN_STYLE.md)
- Compatibility policy: [`docs/COMPATIBILITY_POLICY.md`](docs/COMPATIBILITY_POLICY.md)
- Documentation index: [`docs/README.md`](docs/README.md)
- Writing and publishing documentation: [`docs/DOCUMENTATION.md`](docs/DOCUMENTATION.md); `cargo xtask docs` builds the repository site

## Completed Foundations

- Gate A: Developer experience and language feel
- Gate B: Reliability and regression resistance
- Gate C: Product surface and stdlib quality baseline
- Gate D: Ecosystem and adoption hardening

## Rust Frontend Evaluation (2026-09-05)

Status: Historical bounded prototype checkpoint. Its quality, Rust and documentation gates passed while C was the shipping default (Decision45). The current build and installation select Rust as `fern`; see Rust Migration Completion below.

- [x] Build an independent lexer/parser for a clearly bounded Fern subset (11 parser tests).
- [x] Resolve names and types once into a typed intermediate representation (10 checker tests).
- [x] Generate QBE exclusively from typed IR and reuse the C runtime/backend (17 emitter tests and native execution).
- [x] Add check/emit/build/run commands and specification-grounded differential tests (51 Rust tests, 32 native programs, 5 invalid inputs, literal paths).
- [x] Measure clean/incremental frontend builds, check/emit latency, binary sizes, and native execution.
- [x] Record evidence, gaps, and a migration recommendation before expanding scope ([evaluation](docs/history/RUST_FRONTEND_EVALUATION.md)): continue Rust incrementally; require parity before switching defaults.

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

- [x] Make the native supervisor retry-limit oracle independent of elapsed wall time, retaining exact retry counts and real deadline coverage (complete debug/release/sanitizer protocol matrices pass on macOS/Linux).

- [x] Remove executable-write races from documentation opener tests using immutable scripts and private symlinks; retain literal non-UTF-8 argv and timeout assertions (six complete parallel Linux binary-suite runs pass).

- [x] Restore bounded inline match arms with guarded patterns, canonical formatting and explicit nested/caller comma ownership (37 focused parser/formatter checks, ten native migration programs, 512 unchanged seeded parser/formatter cases per compiler; Decision121).

- [x] Preserve consistent tab indentation, eight-column layout and byte-correct spans while rejecting mixed styles; canonical formatting uses spaces (24 focused parser/formatter tests and Clippy; Decision120).

- [x] Validate documentation snippets with the actual Rust default without rewriting Result obligations or literal source structure; restore current opaque JSON signatures and complete fallible-API examples (eight extractor/default-compiler regressions and docs gate pass).

- [x] Keep cold bootstrap files private under permissive caller masks and inventory inaccessible system-library subtrees without hiding searchable files (both-platform debug/release/sanitizer permission and caller-mask matrices; Linux cold/warm cache regression; Decision119).
- [x] Make the offline Python lock-drift oracle independent of registry cache state, retaining exact lock rejection and an unlocked execution control (25 workflow checks pass).

- [x] Synchronize release version updates across C/Rust and benchmark locks, publish complete readiness bundles, and measure the compiler named in memory reports (four CI/release regressions, actual release-please updater verification, 12 mise tests and three benchmark fixtures).

- [x] Implement actual checked local LSP rename and canonical formatting code actions, with versioned UTF-16 edits, capture rejection and explicit scope bounds (18 action/negotiation regressions, existing LSP checks and fresh-binary protocol smoke; Decision115).
- [x] Inventory and bundle exact third-party license notices for all default Unix Cargo dependencies and compiled native components; locked closure and native-license checks pass.

- [x] Restore CLI literal-path delimiters, default documentation cwd, public fern identity and REPL type/help/clear commands; real PTY editing, completion, persistent history and cancellation pass with bounded history import (Decision115).

- [x] Audit every shipping C builtin name and executable construct against typed Rust lowering (212-name inventory, 48 focused checks, six independent language migration programs and two C-reference outputs; docs/LANGUAGE_PARITY.md).
- [x] Build a tested Rust-default publication path preserving the explicit C reference and prior default on Cargo failure (eight debug/release/helper/artifact-selection/failure workflow cases, including directory destinations; final macOS/Linux acceptance recorded below).

- [x] Disable development component fallback in installed Rust distributions, even with damaged markers (native component-selection regressions; explicit sibling/override behavior retained).

- [x] Require complete default Rust release components and validate exact typed package identity, executable helpers and regular archive members (eight independent distribution tests, exact marker types and owner-executable helpers; nine installation checks include sixteen directory-destination failures).

- [x] Restore 14 shipping service aliases, bracket list indexing and infix membership through typed operations (four parser/checker/formatter/REPL tests and four native programs covering full-width values, Float/NaN, source order and deferred bounds-fault cleanup).

Status: Compiler migration acceptance is complete on macOS arm64 and Linux arm64.
Rust is the default `fern`; `fern-c` remains the explicit C bootstrap/reference.
Executable baseline parity, tooling, native components, installation, packaging,
fuzz and separate compiler performance gates pass. Future language and runtime
features retain their own milestones below.

- [x] Custom algebraic/record types, generic functions, nested patterns and guards (41 checker and 36 emitter tests; native recursive values and guarded matching).
- [x] Modules/imports/visibility and a realistic application spanning multiple files (13 loader tests including visibility bypass regressions, native project execution).
- [x] Verify executable C-baseline language parity through typed Rust operations and independent native outputs, with intentional JSON/Option/error corrections documented (212-name inventory, docs/LANGUAGE_PARITY.md; QBE and Cranelift gates on macOS/Linux).
- [x] Replace reference-only Result checks with bounded reachable-path handling, call/alias provenance, complete collection coverage and deferred cleanup (Decision95).
- [x] Prove direct recursive nominal Result handlers, complete List/Map child traversals, exact recursive callbacks and deferred child handling (Decision95 R1–R2).
- [x] Prove complete mutual structural handler groups over actual nominal descendants, including typed List/Map traversal and transparent wrapped children, with independent source-order and unsafe-path probes (Decision95 R3).
- [x] Verify the combined boundary/CLI/Result checkpoint on macOS and Linux: full Rust/native/C/docs gates, 1447/1449 nextest tests without skips, separate doctests and Criterion smoke.
- [x] Complete bounded recursive builder contracts and mutual-summary widening without unproved handling credit (90 focused integrations, 76 internal obligation checks; independent empty-subtree cardinality exploit rejected; conservative equations documented in docs/RESULT_HANDLING.md, Decision114).
- [x] Verify standard-library/native ABI compatibility and executable applications on both platforms, including runtime debug/release/sanitizer oracles and 305 independent backend outputs.
- [x] Complete diagnostics, formatting, terminal REPL, negotiated LSP edits, documentation and developer-command parity (docs/TOOLING_PARITY.md; real PTY/RPC and literal-source documentation regressions).
- [x] Preserve common quiet/verbose/color controls, literal forwarded argv, visible failures and missing-command status in the Rust CLI (Decision108A).
- [x] Add bounded source-only lex/parse inspection with byte spans, escaped dumps, no import/type/backend execution and atomic parser/limit failure (Decision108B).
- [x] Open retained HTML documentation only after complete atomic generation, with literal platform argv, visible best-effort failures and consistent byte limits (Decision108C).
- [x] Verify the 1006-test expanded Rust checkpoint, C quality gate and documentation on Linux arm64 with Rust 1.75; expose POSIX test APIs under glibc strict C11 without hiding Darwin extensions.
- [x] Package explicit Rust/native inputs as a relocatable opt-in preview, with deterministic bounded archives, atomic publication, strict verification and missing-helper isolation (Decision110).
- [x] Complete Rust-default acceptance on macOS/Linux arm64: full quality, Rust/QBE, Cranelift, documentation, LSP, native style-launcher, fuzz, installation, release packaging and separate Rust/C performance gates pass (docs/RUST_DEFAULT_MIGRATION.md).

Compiler migration acceptance is complete. Historical checkpoints and remaining
language, runtime, editor and backend work below retain their own exit criteria.

### Rust Migration: Expanded Language and Tools

Status: Verified expanded-language checkpoint, incorporated into the Rust default compiler. Remaining planned features are tracked separately from executable baseline migration.

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
- [x] HexDocs-style documentation sites (Decision154): `@moduledoc` through parser, formatter, preflight budget and doctests; a bounded in-tree Markdown renderer with Fern highlighting, safe links and declaration cross-references; `fern doc --site` with summary tables, sidebar, client-side search, themes, guides, `--title`/`--version`/`--link` and atomic source-safe publication; `cargo xtask docs` renders the repository guides, examples and embedded `cargo doc` output (22 new Rust regressions across `moduledoc`, `documentation_markdown`, `documentation_site` and `documentation_site_cli`, plus a generated-method omission case in `documentation_inferred`; the 2026-09-14 macOS ARM64 `cargo xtask check` passes formatting, notices, workspace Clippy, 2,261 Rust tests across 278 suites, 316 native fixtures, 20 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192 fuzz cases; light/dark module, guide and search pages inspected in a real browser).
- [x] Whole private-signature inference and SCC generalization, including delayed shape constraints.
- [x] Core private-signature generalization from patterns and bodies, with callee-first recursive components, rigid annotations and intrinsic requirements (eight native programs, 14 invalid programs and three interactive regressions).
- [x] Delayed shape obligations using later body evidence for fields, updates, tuple-rest and iteration; non-executable probes rejected at publication boundaries (seven native programs, eight invalid programs and two interactive regressions).
- [x] Typed hover and valid-source record/tuple/member completion from final checked source facts, with instantiated uses, inferred requirements, exact doc ownership and separate type/value namespaces (26 new metadata/editor regressions).
- [x] Incomplete member completion from independent concrete receiver evidence in current source, with exact edits and no executable hole IR (22 new Rust regressions including library source identity).
- [x] Preserve global module reference identity when an unrelated local shadows its canonical module name (eight Rust regressions, four native programs and four invalid inputs).
- [x] Transparent scalar/generic type aliases with bounded capture-free expansion, module privacy, original source tooling and native execution (26 new Rust regressions, seven native programs and 12 invalid inputs).
- [x] Distinct generic newtypes with unboxed full-width native representation, explicit projection/patterns, scalar capabilities, Result obligations and source tooling (45 Rust regressions, 13 native programs and 14 invalid cases).
- [x] Independent module type/value declarations and visibility, selected/wildcard reexports, lexical receiver resolution and exact source navigation (19 Rust regressions, 12 native programs and 12 atomic invalid cases).
- [x] Canonical finite unions, contextual member/subset conversion, typed narrowing and full-width native/REPL carriers, preserving generic equality and Result obligations (87 Rust regressions, 27 native programs, 23 atomic invalid cases and unit/main entry checks).
- [ ] Union constructor refinements, variance, implicit joins and lifted capabilities.
- [ ] Remaining function/type syntax and complete native-language audit.
- [x] Track Result duties beyond local references; metadata, partial searches and incomplete branch handling do not acknowledge entire values (Decision95).

### Rust Migration: Collections and Error Values

Status: Complete and incorporated into the Rust default compiler. Decisions 45–46 describe the historical collections/error-value checkpoint; see [completed migration acceptance](docs/history/RUST_DEFAULT_MIGRATION.md).

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
- [x] Retire Tree-sitter integration under the explicit Rust-only scope; retain Rust LSP. Historical grammar parity/publication work is no longer planned.
- [x] Execute bounded Rust native actor functions with typed mailboxes, selective receive/timeouts, explicit continuation frames, invocation-owned quotas and fault cleanup (Decision105A; native20-program and independent lifecycle/sanitizer gates).
- [x] Suspend direct recursive helpers and List/Map/Range loops through typed return frames, preserving operand order, lexical exits and ordinary native entry points (Decision138).
- [x] Preserve logical actor defer scopes across suspension, faults and cancellation; validate LIFO cleanup, original-fault precedence, precise roots and bounded retained state with native/REPL tests and a 64-by-256-operation stack model (Decision141).
- [x] Execute checked source actors in persistent REPL sessions and replay bounded virtual-time transcripts through FernSim, including selective receives, deadlines, supervision, stale identities and explicit cleanup (Decision140).
- [x] Compose typed receiving returns, transitive mailbox inference, with/? and ordinary captured callbacks; suspend six List and four Option/Result combinators while preserving Result provenance, selection order and cleanup. Native/REPL/GC and independent seeded models pass (Decision142).
- [ ] Implement external-event/instruction fairness, first-class actor-effect callbacks and typed ancestor escalation/descendant subtree reconstruction beyond the documented continuation boundaries.
- [x] Immutable native JSON parser/accessors/stringifier with exact numbers, Unicode validation and bounded resources (14,309 API checks, 24 budget checks and 6,000 numeric oracle cases in debug/release/sanitizer builds).
- [x] Migrate Rust native JSON to opaque values/errors, immutable builders and bounded lossless collection adapters (ten native programs, twelve semantic rejections, eight Rust integration tests and 248 native builder checks per build).
- [x] Evaluate dynamic JSON in the Rust REPL with exact native semantics, independent cleanup budgets and bounded shared storage (22 new Rust regressions and 12,000 numeric/formatting oracles).
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
- [x] Distinguish JSON unions through nested required record fields, tuple positions and shared-tag sum payloads under shared proof/runtime limits. Six compiler/REPL groups, independent seeded native selection/precise-GC tests and an expected-output native fixture pass (Decision137).
- [x] Execute custom Json implementations and generic bridges in native and REPL structural codecs, with opaque wire profiles, composed error paths, shared callback quotas and original-fault cleanup. Independent callback ABI, forced-GC and seeded full-width round trips pass (Decision139; [custom JSON](docs/CUSTOM_JSON.md)).
- [x] Reassess QBE/Cranelift using current primary sources and an independent native AOT experiment; correct the original QBE rationale (Decision109).
- [x] Trial Cranelift through shared lowering and promote it as the sole native backend under the Rust workspace migration (Decision122). ARM64 native/ABI and recorded performance acceptance pass; source debugging, controlled optimization comparisons and x86-64 execution remain separate work.
- [x] Extract shared typed machine lowering and replace handwritten QBE helper text with structured Rust builders; preserve existing QBE boundary tests (Decision112).
- [x] Add canonical native signatures, fixed Float runtime boundaries and explicit Cranelift build/run selection; selected builds emit objects without invoking QBE or an assembler.
- [x] Preserve the complete default Rust/QBE gate on macOS/Linux ARM64 after shared lowering (1,548/1,551 Cargo tests, native and packaging/workflow oracles, 192 fuzz mutations).
- [x] Revalidate the full C quality and documentation gates on macOS/Linux ARM64; add Cranelift native acceptance to the Linux/macOS CI matrix with bounded test artifact profiles.
- [x] Pass the complete Cranelift feature gate on macOS/Linux ARM64: 293 independent native output oracles per platform, 1,559/1,562 Cargo tests, feature Clippy, and mixed-argument ABI, relocation, phi and forced-GC regressions. Source debugger, controlled performance, x86-64 and default-promotion gates remain separate.
- [x] Reserve an ABI-permitted Apple arm64 QBE scratch register, verify swaps/calls/spills and independent native outputs, and preserve generic Linux assembly (Decision104).
- [x] Remove newly written executable races from native capture tests; preserve timeout/stream/descendant coverage and add 160 concurrent per-run output/status assertions.
- [x] Retain direct-child ownership through native unit/doc test cleanup and validate the framed safe-Rust adapter (Decision102).
- [x] Verify default-command migration to Rust on macOS/Linux, retaining `fern-c` as the explicit bootstrap/reference and documenting its legacy JSON contract (Decision96; docs/RUST_DEFAULT_MIGRATION.md).
- [ ] Expose HTTP serving to compiled Fern applications beyond the Rust preview server, and implement the broader SQL query/resource APIs described in the design.
- [x] Implement coherent static traits, defaults and parent bounds, explicit where requirements, generic implementations and structural Show/Eq/Ord/Clone derivation. Native, REPL, WASM and deterministic semantic oracles pass (Decision132; [traits](docs/TRAITS.md)).
- [ ] Complete the remaining syntax audit and ergonomic inference refinements beyond the documented traits, function, alias/newtype and union contracts.
- [x] Implement nominal immutable Sets with 13 APIs, membership syntax, generic/alias/first-class support, native/REPL model-based simulations and forced precise-GC collection at map helper boundaries; see [Sets](docs/SETS.md).
- [x] Evaluate closed compile-time constants using bounded pure Fern execution, embed typed data, preserve public/module/editor conventions and reject effects even in unused initializers. Seeded arithmetic, aggregate/closure, visibility and actual native-output tests pass (Decision133; [Comptime](docs/COMPTIME.md)).
- [x] Implement source-level native C FFI with exact scalar ABI, checked narrowing, sealed pointer handles, retained string owners and literal library linking. Independent ABI, seeded conversion, module/CLI and forced-GC tests pass (Decision136; [FFI](docs/FFI.md)).
- [x] Extend WASM with captured/indirect functions, maps/sets, collection callbacks, ranges/loops, unions, Result handlers and fault-aware deferred cleanup. Pass 34 Wasmi and four CLI tests, including seeded map, closure/GC and cleanup oracles (Decision135; [portable language](docs/WASM_LANGUAGE.md)).
- [x] Add non-faulting `List.at/first/last/take/drop` and exact `Int.parse` with full-width heap Options; independent runtime ABI, native output, REPL and symbol-inventory tests pass (Decision150; [stdlib reference](docs/STDLIB_API_REFERENCE.md)).
- [x] Name the nearest known symbol in unknown name/function/field/type/module-member diagnostics, list missing constructors in exhaustiveness errors and retire "Rust prototype" parser wording (Decision149).
- [x] Add `List.sort/zip/range/sum` and `Int.checked_add/sub/mul/div/rem/neg`; report argument type mismatches before missing labels. Independent runtime ABI, native output (`sequences/utilities.fn`, `numeric/checked.fn`), REPL and inventory tests pass (Decision151).
- [x] Print lists, maps, options, results, tuples and `derive(Show)` types directly through `println` with an on-demand trait prelude; missing `Show` reports the type with a derive hint (Decision152; `traits/printing.fn`).
- [x] Sort structured and generic elements through `Ord` (`List.sort`) or an explicit comparator (`List.sort_by`) with a stable runtime-driven merge that never crosses the callback ABI; render nested strings as quoted literals (`String.quote`) natively, in the REPL and in wasm; print generic parameters through `Show`. Independent runtime state-machine, native output (`sequences/sort_by.fn`, `traits/printing.fn`), REPL, Wasmi and checker tests pass (Decision153).
- [ ] Complete the specified standard modules, including data formats, testing/utilities, IO/system, cryptography and compression.
- [ ] Audit all specified syntax and stdlib calls for complete typecheck-to-native behavior; reject unsupported execution paths.
- [ ] Complete precise actor memory, fair scheduling and full typed browser application support above. Inferred ownership/reuse remains an optimization, not a mandatory application borrow checker (Decision124).

## Next Session Start Here

The Rust-only workspace migration is complete within its recorded acceptance
scope. The active direction is the full-stack actor/browser plan above. Continue
using the Rust runtime and Cranelift through `cargo xtask check`; historical
compiler-default evidence does not validate new runtime or browser features.

1. Complete native root/layout coverage and resumable actor continuations; establish independent fairness, actor failure and resource-cleanup oracles before adding workers. Preserve full-width values and bounded lifecycle contracts.
2. Extend the aggregate WASM backend beyond the tested application surface; derive shared wire schemas and add application-independent framework packaging. Preserve actual Fern model/update/view and native actor acceptance.
3. Preserve the working static/offline application, fixed-owner clusters and durable room recovery while extending measured progress and transactional external-effect recovery. Treat dynamic membership and replicated failover as separate milestones.
4. Close supporting language/stdlib gaps in [release readiness](docs/RELEASE_READINESS.md), maintain the Rust LSP, and collect fresh native/browser correctness and performance evidence. Tree-sitter remains removed.
