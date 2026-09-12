# Fern release readiness

Fern is pre-1.0. The compiler, native runtime, supervisor and repository tooling
are implemented in Rust. Native compilation uses Cranelift. The Rust LSP remains
available; Tree-sitter and the old C/QBE/bootstrap setup have been removed.

The supported language includes checked modules, generics, aliases/newtypes,
finite unions, closures, collections, Result handling, derived JSON codecs and
bounded cooperative actors. File/process APIs, SQLite lifecycle operations,
HTTP clients, regex and terminal widgets have executable regression coverage.
See the [standard library reference](STDLIB_API_REFERENCE.md) and [roadmap](../ROADMAP.md).

## Remaining language work

The active product direction is supervised native actors plus a reactive Fern
WebAssembly browser application over a typed WebSocket protocol (Decision124).
The [full-stack architecture](FULL_STACK_ARCHITECTURE.md) defines its staged
acceptance. Actor-owned heaps, fair resumable scheduling, external-event liveness,
typed supervision, a browser ABI/backend and bounded reconnect semantics must be
proved before claiming an Elixir/Phoenix alternative. The first browser/server
demo is single-node and explicitly ephemeral; durability and distributed recovery
are separate gates. Generated browser interop is permitted build output, while
implementation remains Rust-authored and application logic remains Fern.

The rewrite preserves supported behavior; it does not implement every proposal
in [DESIGN.md](../DESIGN.md). Open work includes generalized actor suspension and
typed supervision, actor REPL parity, deeper JSON union discrimination and custom
traits, HTTP serving, typed SQL query APIs, Sets and remaining standard modules,
advanced ownership/reuse analysis and a WASM backend. Unsupported forms must keep
explicit diagnostics. A successful typecheck alone does not certify execution.

The native collector is Rust-owned tracing GC. SQLite uses rusqlite's bundled
library; HTTP uses ureq/rustls with certificate verification and Rust-wrapped
crypto. Third-party native libraries remain permitted. Host linkers and SDKs are
required, and generated executables are not promised to be universally static.

## Verification

```sh
cargo xtask check
cargo xtask check --release
cargo xtask fuzz 512 0xC0FFEE
mise run rust-bench-smoke
cargo xtask package
```

The quality gate includes workspace tests, strict formatting/Clippy, native output
fixtures, examples and fuzz invariants. It exercises real terminal and LSP process
behavior as well as runtime/supervisor lifecycle boundaries. Packaging verifies
closed archive membership, checksums, file types and permissions; installation
uses anchored directory descriptors and atomic component replacement.

Linux and macOS jobs must pass. ARM64 acceptance does not establish x86-64 runtime
acceptance; run on each architecture before publishing that architecture's bundle.
Performance reports must identify the actual compiler/runtime artifacts and host.
Historical C/QBE timing or size budgets do not measure the new Cranelift binary.
Source debugger support and controlled cross-architecture performance remain
separate gates. No tag, registry publication or 1.0 release is implied by local
rewrite acceptance.
