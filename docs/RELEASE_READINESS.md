# Fern release readiness

Fern is pre-1.0. The compiler, native runtime, supervisor and repository tooling
are implemented in Rust. Native compilation uses Cranelift. The Rust LSP remains
available; Tree-sitter and the old C/QBE/bootstrap setup have been removed.

The supported language includes checked modules, generics, aliases/newtypes,
finite unions, closures, immutable Sets, compile-time constants, static traits and
explicit bounds, structural and custom JSON codecs, Result handling and bounded
cooperative actors. Portable language values also execute through the WASM backend. File/process APIs, SQLite lifecycle operations,
HTTP clients, regex and terminal widgets have executable regression coverage.
See the [standard library reference](STDLIB_API_REFERENCE.md) and [roadmap](../ROADMAP.md).

## Remaining language work

The active product direction is supervised native actors plus a reactive Fern
WebAssembly browser application over a typed WebSocket protocol (Decision124).
The [full-stack architecture](FULL_STACK_ARCHITECTURE.md) defines its staged
acceptance. The current checklist runs Fern domain actors and a typed Fern browser
model/update/view, with bounded reconnect behavior and optional local durable room
checkpoints. Worker sharding, actor-owned heaps and [fixed-owner server clusters](CLUSTER.md)
are implemented and have independent real-process stress/fault tests. General
external-event liveness, complete preemption, subtree supervision, replicated
ownership and distributed recovery still need separate acceptance. Generated
browser interop is permitted build output; authored implementation remains Rust
and application logic remains Fern.

The implementation does not cover every proposal in [DESIGN.md](../DESIGN.md).
Direct actor helper recursion and loops now use typed continuation frames; see
[actor continuations](ACTOR_CONTINUATIONS.md) for the exact scheduling and cleanup
boundaries. [Traits](TRAITS.md), [Sets](SETS.md), [compile-time constants](COMPTIME.md),
[native FFI](FFI.md), [deep JSON unions](JSON_TYPED_CODECS.md),
[custom JSON methods](CUSTOM_JSON.md) and the [portable WASM subset](WASM_LANGUAGE.md)
have focused execution tests. HTTP framework packaging, broader SQL query APIs,
remaining standard modules, advanced ownership/reuse optimization and the complete
syntax/target parity audit remain open. Unsupported forms must keep explicit
diagnostics. A successful typecheck alone does not certify execution.

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
