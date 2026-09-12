# Compatibility and deprecation policy

Fern compatibility covers executable language syntax, type checking, standard
library APIs and public CLI behavior. `DESIGN.md` includes future work; parsing
or checking a proposed construct does not establish executable support.

## Versioning and changes

The workspace version in root `Cargo.toml` is the version source of truth.
Release-please updates it, the local package entries in both Cargo lockfiles,
`.github/release-version.txt`, its release manifest and the changelog. Release
packaging rejects a tag that disagrees with the workspace version.

Patch releases preserve compatibility. Minor releases may add compatible
features. Breaking releases need explicit migration notes and the appropriate
major-version change. Experimental features must be identified as such. An
ordinary deprecation names its replacement and earliest removal release, with
at least two minor releases between announcement and removal.

The unreleased Rust migration is an explicit source and implementation change:

- The compiler, runtime, supervisor and maintenance tools are Rust. Cranelift
  produces native objects; the C reference compiler, QBE and Tree-sitter package
  are removed. The Rust LSP remains available through `fern lsp`.
- JSON uses opaque `json.Value` and `json.Error`. The old String-copy JSON API
  and its native entry points are removed. Parse text before stringifying a
  JSON value, or use `json.from_string` to construct a JSON string.
- `fs.list_dir` and its `File` alias return `Result(List(String), Int)`. Match the
  Result or propagate it from a compatible function. Empty directories return
  `Ok([])`; errors never publish a partial listing.
- Rust preserves full-width values, checked Result duties and explicit faults.
  Known miscompilation and unsafe or unbounded old behavior are not compatibility
  requirements. Decision123 documents bounded service corrections.

A release containing these changes must describe them in its migration notes.
The completed compiler-default checkpoint is historical evidence; acceptance of
this workspace is recorded separately in [Rust workspace acceptance](RUST_WORKSPACE.md).

## Library contracts

Prefer lowercase service modules `fs`, `json`, `http`, `sql` and `actors`.
Core utilities use `String`, `List`, `System`, `Regex`, `Result`, `Option` and
`Tui.*`. Existing aliases remain supported; an alias removal follows the same
published deprecation policy. Function signatures are maintained in the
[standard library reference](STDLIB_API_REFERENCE.md).

- [JSON](JSON_RUST_API.md) validates Unicode and structure, preserves number
  spelling and returns stable ordinary errors. [Typed codecs](JSON_TYPED_CODECS.md)
  retain their documented type, path and resource limits.
- HTTP returns response text for successful 2xx responses. Invalid URLs,
  redirects, other statuses, certificate failures, transport errors and invalid
  text return errors. ureq/rustls verifies certificates; calls have a 30-second
  deadline and a 16 MiB response limit.
- [SQLite](SQL_LIFECYCLE.md) uses rusqlite with bundled SQLite. Closing releases
  locks and unfinished transactions; stale handles never become valid again.
  The 256-live-connection limit rejects an additional open before filesystem
  effects. Query APIs and remote databases remain separate planned work.
- [Mailbox supervision](ACTOR_RUNTIME.md) preserves FIFO messages, permanent dead
  identities, single-use restart lineage, ownership forests and documented restart
  policies. [Typed native actors](RUST_ACTORS.md) have a separate bounded
  cooperative execution contract. Neither API promises parallel workers or the
  complete planned actor model.
- [Memory management](MEMORY_MANAGEMENT.md) uses the Rust tracing collector.
  Explicit dup/drop metadata is retained where specified; it is not a claim that
  precise reference counting or complete ownership inference is implemented.

Native ABI callers must meet documented pointer, lifetime and layout requirements.
The Rust runtime retains required native calling conventions; a C ABI does not
mean that Fern contains an authored C implementation. Third-party native libraries
behind Rust wrappers are permitted, with a preference for native Rust dependencies.

## Release checks

Before publishing a tagged release:

1. Run `cargo xtask check` and the additional target-specific checks in
   [release readiness](RELEASE_READINESS.md).
2. Build the release, measure the actual artifacts with `cargo xtask perf report.json`, and
   inspect the reported host, sizes, timings and component hashes.
3. Run `cargo xtask package`, verify its archive/checksum and execute relocated
   build/run/source-test checks on each architecture being published.
4. Include compatibility changes, deprecations and migration guidance in release
   notes. Normal tags and notes are produced by release-please; manual recovery
   remains exceptional.

`.github/workflows/ci.yml` and `.github/workflows/release.yml` run Rust workspace
acceptance. Repository tests check the workflow and version-update contracts.
These checks establish executable evidence; they do not automatically approve a
release or replace review of its compatibility notes.
