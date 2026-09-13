# Runtime memory

Fern keeps ordinary application memory management automatic. Native values use
a nonmoving tracing collector implemented in Rust. The browser backend has its
own representation and a bounded precise String heap. Neither target requires
application borrow checking, move syntax or lifetime annotations.

## Native ownership

An invocation thread owns the runtime heap registry. Invocation control data and
actor identities live in the invocation heap; each actor has an independently
owned payload heap. Actor callbacks enter their heap through a scoped guard and
restore the previous heap on return. Retiring an actor reclaims its payload heap
after active scopes have finished. A collection traces only the active heap.

Spawned captures and messages are copied into receiver-owned storage. Copying
preserves sharing within a supported graph, roots intermediate allocations and
publishes only a completed copy. Shape, traversal and logical resource checks
precede publication. Rejected sends do not expose a partial payload. These bounds
do not make host allocator exhaustion recoverable; an allocator failure can
still terminate the process.

The native ABI remains in `crates/fern-runtime/src/abi.rs`. PIDs identify
invocation-owned actors; copying a PID does not transfer actor-control storage.
Actor-owned payload storage
does not make the current scheduler multicore or provide failure isolation:
callbacks remain cooperative, and recoverable actor fault handling still needs
to replace invocation-wide termination paths. See [actor contracts](RUST_ACTORS.md).

## Roots and collection

The compiler emits explicit root frames for managed values. Runtime root guards
also protect Fern pointers held only in Rust temporary buffers while allocations
or callbacks can collect. Roots remember their owning heap and cannot move
between threads. Actor control and suspended state retain their payload roots
through the registered ownership boundary.

Ordinary collection still scans supported native stack/register state and
pointer-sized words in non-atomic allocations. The address index recognizes
interior pointers; atomic string/data allocations are not scanned. Explicit
compiler roots are a foundation for precise collection, not evidence that the
whole native runtime is now precise. The precise-root collection entry point is
an unsafe test/runtime boundary; it requires every live reference to be registered.
Removing conservative scanning requires a complete allocation and root audit,
including compiler helpers, callbacks, suspended state and precise heap layouts.

Rust-owned JSON graphs use finalizers and capacity-aware retained-byte accounting.
Cross-actor copies own independent JSON graphs. A finalizer runs once on
reclamation or heap shutdown; it must not panic or reenter the collector.
Native startup remains isolated in `fern-runtime-native`, so Rust tests can link
the core runtime without a second program entry point.

Reference-count metadata remains part of the ownership bookkeeping ABI.
Explicit release helpers do not invalidate live aliases; reachability determines
lifetime. This is not a Perceus allocation-reuse implementation. Actor quotas
remain logical resource limits separate from collector accounting.

Platform stack/register code covers macOS/Linux on ARM64 and x86-64. The recorded
native migration acceptance ran on ARM64. Static x86-64 web-server validation
does not validate the native Fern collector or compiler on that architecture.

## WebAssembly ownership

The separate WebAssembly emitter consumes checked semantic IR before native
pointer lowering. Language integers remain i64; browser pointers are i32.
Scalar programs need neither memory nor host imports. Programs using the supported
strings or aggregates use a bounded nonmoving linear-memory heap and compiler
shadow roots. Records, tagged sums, Option/Result, lists and tuples have precise
child-pointer maps; integers that resemble addresses do not retain objects.
Browser collection never depends on native stack/register scanning. Captured
closures, maps and actors remain unsupported by this backend and reject before
an output module is published.

The checklist keeps its complete Fern model and view in that heap. Its Rust WASM
host owns type-checked positive i64/BigInt handles, with 55-bit nonwrapping
generations and explicit release. Bounded UTF-8 scratch transfer copies strings;
raw Fern pointers never cross into host code. The modules do not share a heap.
Host listeners, timers and socket callbacks have explicit cleanup, and unmount
releases model handles and subscriptions. The Rust service worker owns the offline
asset cache. See the [web preview guide](WEB_PREVIEW.md).

## Remaining memory work

Decision124 keeps actor-owned tracing as the server direction. Compiler-inferred
ownership, borrowing and reuse may optimize allocations without changing ordinary
language semantics. Shared immutable binaries may later use explicit reference
counting with ownership and byte accounting.

Per-actor heaps alone do not bound scheduler latency. Collection, copying, mailbox
search and execution must share measured work budgets, with resumable
continuations keeping roots valid at every yield. Precise native heap layouts,
complete root coverage, multi-worker ownership and reliable host-resource
cancellation remain acceptance gates. The browser ABI has independent nested-value, stale-handle and pressured-root
execution tests; broader capabilities and a WasmGC comparison remain open before
stabilization.

Run `cargo test -p fern-runtime` for runtime checks and `cargo xtask test` for
native integration. WASM execution tests live in the compiler crate. See the
[full-stack architecture](FULL_STACK_ARCHITECTURE.md), [roadmap](../ROADMAP.md)
and [historical memory plan](history/MEMORY_MANAGEMENT_PLAN.md) for scope and history.
