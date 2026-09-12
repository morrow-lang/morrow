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
