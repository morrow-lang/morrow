# Runtime memory

Fern's native runtime uses a nonmoving tracing collector implemented in Rust.
Each invocation thread owns its heap. Generated code uses the native ABI in
`crates/fern-runtime/src/abi.rs`; the compiler emits matching layouts and calls.

The collector keeps an address index and recognizes interior pointers. At
collection points it scans the supported host stack and preserved registers,
then follows pointer-bearing allocations. Atomic string/data allocations are
not scanned. Rust temporary buffers containing Fern pointers require an explicit
root guard while allocations or callbacks can trigger collection. Guards cannot
move between threads.

Rust-owned JSON graphs use finalizers and capacity-aware retained-byte accounting.
Each finalizer runs exactly once on reclamation or invocation shutdown. It must
not panic or reenter the heap. Native startup is isolated in
`fern-runtime-native`, allowing Rust tests to link the core library without a
second program entry point.

Reference-count metadata remains an ownership bookkeeping ABI. Explicit release
helpers do not invalidate live aliases; reachability determines lifetime. This
is not the proposed Perceus reuse optimization. Actor quotas remain explicit
invocation-owned logical limits, separate from collector heap accounting.

Collector platform code covers macOS/Linux on ARM64 and x86-64. Acceptance in
this rewrite was executed on ARM64; x86-64 runtime validation remains required
before publishing those bundles. Platform stack
and register code is isolated under `crates/fern-runtime/src/memory/`. Tests cover
interior roots, cycles, allocation pressure, finalized Rust values and a live
pointer held only by Cranelift-generated code across collection.

Run `cargo xtask test` for native integration and `cargo test -p fern-runtime`
for runtime tests. See [the previous memory plan](history/MEMORY_MANAGEMENT_PLAN.md)
for historical proposals, and [the roadmap](../ROADMAP.md) for remaining work.

## Adopted direction: actor isolation and browser execution

Decision124 prioritizes actor-owned tracing heaps with precise roots over a
universal reference-counting replacement. Ordinary Fern code keeps automatic
memory management. Compiler-inferred ownership, borrowing and reuse may optimize
it without adding mandatory lifetime or move rules to the language.

The server target gives each actor an independently reclaimable heap and copies
message/capture graphs across ownership boundaries. GC, copying and mailbox work
must be budgeted alongside resumable execution. Shared immutable binaries may
later use explicit reference counting. The present TLS heap and retained message
pointers do not implement this isolation and must not simply be marked thread-safe.

The initial browser target is wasm32 linear memory with a precise collector and
compiler-maintained roots, preserving 64-bit language integers while separating
pointer and handle representations. WasmGC must be evaluated before stabilizing
the browser ABI. Native stack/register scanning cannot discover browser locals.
Both targets require explicit host-resource ownership and callback cleanup.

These are adopted design requirements, not changes already made to allocation or
collection. The [full-stack architecture](FULL_STACK_ARCHITECTURE.md) defines
migration gates and the [roadmap](../ROADMAP.md) tracks their implementation.
