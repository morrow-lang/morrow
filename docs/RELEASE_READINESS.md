# Fern release readiness

Fern is pre-1.0. The following describes executable behavior, not every feature in
[the language design](../DESIGN.md). Historical Gate A–D completion records
engineering milestones; they do not certify the entire language.

## Available and regression covered

| Surface | Current behavior | Verification |
| --- | --- | --- |
| First program | Check, format, compile, run; relocatable compiler/runtime pair; local install | Installation integration tests and tutorial output assertions |
| Native strings | Quotes, backslashes, control bytes, Unicode, long literals; typed user-function print results | String and print codegen execution regressions |
| Files | Bounded complete UTF-8 read/write/append with Result errors, plus delete/size | Native fault/limit tests on macOS/Linux and interactive text tests |
| HTTP | GET/POST clients, response bodies on 2xx, integer errors otherwise | Local HTTP/TLS runtime tests; offline error example |
| SQLite | Open, execute statements and explicitly close; 256 live connections with stale-handle protection | Native lifecycle/quota/transaction tests through C and Rust, plus sanitizers |
| Actor foundation | String FIFO mailboxes, lifecycle/monitor/restart, three deterministic strategies | Six invariant scenarios and 1,536 seeded strategy crash steps |
| Rust native actors | Typed Pid, cooperative execution, selective receive/deadlines and bounded continuation frames | Twenty native programs, semantic rejections, independent IR and lifecycle/sanitizer gates |
| Rust preview | Opt-in relocatable bundle of explicit compiler, helper and runtime inputs; C remains default | Bounded archive tests and moved-package execution gates |
| Terminal UI | Styled output, panels/tables, editable input/password prompts, cursor controls, immutable trees, logs | 13 native/PTY tests and a compiled example |
| Editor | Rust LSP, bounded Tree-sitter corpus and locally staged Zed extension | Native/WASM source parity, reproducible package tests and isolated actual-Zed LSP startup |
| Native checker | Default C-bootstrap cached launcher; ordinary style checks need no Python/Cargo | 66 independent workflow cases, exact diagnostics, cache/concurrency/ownership and sanitizer gates on macOS/Linux |

## Blocking full language completion

- **Complete concurrency:** the opt-in Rust frontend executes bounded typed actors
  with selective receive, timeouts and explicit continuation frames. Generalized
  suspension, typed ancestor escalation and descendant subtree reconstruction,
  actor REPL/FernSim parity and the planned million-step reliability target remain
  open. The default C frontend retains explicit mailbox/supervision primitives
  and rejects execution syntax. See [Rust native actors](RUST_ACTORS.md) and
  [the legacy actor contract](ACTOR_RUNTIME.md).
- **JSON:** Rust native execution and its REPL use the bounded, validating opaque
  JSON model with exact numbers and immutable builders. The legacy C source API
  still copies strings and can accept invalid JSON. Explicitly derived record codecs
  now include regular recursive schemas with finite bases and transparent
  newtypes, tagged sums, disjoint unions and conditional generic codec requirements.
  Deeper union discrimination, general traits and the verified Rust default
  switch remain open; see [typed codecs](JSON_TYPED_CODECS.md) and
  [the Rust JSON contract](JSON_RUST_API.md).
- **Server and database APIs:** HTTP serving, typed SQL queries and the broader
  design-level application stack are not implemented by the current client and
  SQLite execute primitives.
- **Result handling:** the Rust checker proves reachable-path handling through
  aliases, collections, generic calls and deferred cleanup. Metadata-only uses,
  partial searches and uncovered early exits reject. Direct recursive nominal handlers,
  complete child-collection traversals and mutual structural handler proofs are
  supported; general recursive builders and wider summary equations remain incomplete. See
  [the handling contract](RESULT_HANDLING.md).
- **Editor completeness:** the verified grammar corpus is bounded. Source-label completion supports closed and EOF-open calls; remaining syntax
  and broader malformed-source recovery remain open. Local Zed packaging does not publish its pinned grammar revision.
- **Memory and targets:** Boehm GC remains the native memory backend. Ownership
  primitives are a baseline, not complete Perceus analysis. WASM is planned.
- **Language coverage:** every supported design construct still needs a complete
  parse/check/native-output audit. The existing examples cover a useful subset,
  and successful type checking alone does not certify executable semantics.

## Release verification

Before tagging a release, run these from a clean checkout with documented native
dependencies installed:

```sh
mise run check
mise run style-parity
mise run docs-check
mise run fuzz-smoke
mise run lsp-rpc-smoke
mise run release-policy-check
mise run perf-budget
mise run release-package
mise run release-package-check
```

`mise run check` includes native user workflows, installation, PTY, string/print, and
actor regression coverage. `mise run perf-budget` measures a release build; its
budgets are enforced in the script, not inferred from aspirational README sizes.
`mise run release-package` builds the release bundle, and its packaging script verifies
the archive checksum and required members.

The supported CI matrix is Linux and macOS. Local validation on one host does
not substitute for both CI jobs. Compilation requires a host C compiler, GC,
SQLite, and OpenSSL development libraries; compiled programs may retain platform
shared-library dependencies. A release must not advertise universal static
portability without checking its actual linked dependencies.

A 1.0 proposal must close the blocking items above, document compatibility and
migration behavior, and demonstrate real application execution under the
[compatibility policy](COMPATIBILITY_POLICY.md). See [ROADMAP.md](../ROADMAP.md)
for the current task list and [the language guide](LANGUAGE_GUIDE.md) to get started.
