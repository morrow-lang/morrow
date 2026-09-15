# Sendable actor heaps and the explicit execution context

Status: proposed. Date: 2026-09-15. Step 1 of 5 toward parallel actor execution.

## Why

Morrow's actor scheduler executes on one thread. `docs/ACTOR_RUNTIME.md:25` states
that "the scheduler executes FIFO continuation callbacks on one thread", and
neither actor contract promises parallel workers. The web host reaches parallelism
by pinning independent whole runtimes to threads so that native PIDs never cross
them (Decision128, `docs/WEB_WORKERS.md`), which is sharding rather than a parallel
scheduler.

The BEAM comparison in `benchmarks/language-comparison/BEAM.md` records that actor
throughput, scheduler fairness and fault recovery against BEAM remain unmeasured.
A language that positions its actors against Elixir cannot leave its scheduler on
one core indefinitely, and the ownership model that blocks parallelism is the
hardest thing in the runtime to change once more code depends on it.

This step changes no observable behavior. It moves heap ownership out of
thread-local storage so that later steps can run schedulers in parallel.

## The five steps

1. **Sendable actor heaps and the explicit execution context.** This document.
2. **Multi-scheduler execution.** Per-scheduler run queues, a global PID registry,
   cross-scheduler send. Actors pinned to their spawning scheduler.
3. **Preemption.** Compiler-inserted yield checks at loop back edges and calls in
   the Cranelift and WASM backends, plus a scheduler quantum.
4. **Work stealing and heap migration.** Actors move between schedulers.
5. **Thread-affine resource policy.** Foreign handles currently operate only on
   their opening thread (`docs/ACTOR_RUNTIME.md:70`); migration breaks that.

Each step is required. Splitting them is sequencing, not deferral: step 1 alone
produces no parallelism and must therefore be judged on whether it makes step 2
possible without a second ownership rewrite.

## Current architecture

Verified in the tree at commit `bd1a5e0`:

- `crates/morrow-runtime/src/memory/heaps.rs:66` declares
  `thread_local! { static STORE: RefCell<Store> }`.
- `Store` (`heaps.rs:23`) owns `slots: BTreeMap<usize, Slot>`, each `Slot` owning
  its own `Heap` with independent `roots`, `blocks` and `collections`; an `active:
  usize` cursor selects the heap that allocation currently targets.
- `heaps.rs:68-79` routes every allocation through `with`/`with_mut`, which resolve
  `store.slots[&store.active].heap`.
- Slot `0` is the **invocation heap** (`Slot::invocation()`), shared by every actor
  on the thread.
- `heaps.rs:86` `control_edge` asserts that an actor's PID control pointer is an
  allocation in `slots[&0].heap.blocks`, and `remove_retired` (`heaps.rs:50`)
  removes that control root from slot 0 when an actor retires.
- `Store.frames: Vec<Frame>` holds root frames keyed by heap id (`frame.heap`).
- `memory.rs:213` and `heaps.rs:182` carry `_thread: PhantomData<Rc<()>>`,
  making the heap types `!Send` and `!Sync` deliberately. Decision at
  `DECISIONS.md:211` records the intent: the server owner thread constructs the
  native domain "without Send implementations for its heaps".
- `crates/morrow-runtime/src/actors/api.rs:3` holds the compatibility mailbox
  scheduler in a second thread-local, `STATE`.
- `crates/morrow-runtime/src/managed/scheduler.rs` runs a FIFO intrusive list of
  `*mut Actor` owned by a `Session`, reached through `(*a).exec.session`.
- The runtime exposes 257 `#[unsafe(no_mangle)]` entry points; every one that
  allocates or touches a managed value resolves its heap through the ambient
  store. `crates/morrow/src/runtime.rs` and
  `crates/morrow/src/runtime_abi.rs` define the compiler's view of that symbol set.

Two facts follow. Per-actor heap isolation **already exists** — the thread-local is
a container of many heaps plus a cursor, structurally the same shape as BEAM's
current-process pointer. And the obstacle to migration is **not** the `PhantomData`
marker but the shared invocation heap: actor payload heaps hold control edges that
point into slot 0.

## Goal

Heap ownership moves from thread-local storage to an explicit, movable owner, so
that a later step can run two schedulers at once without a second rewrite.

Concretely, at the end of step 1:

- An actor's payload heap is owned by that actor and is `Send`.
- Domain-scoped state — the invocation heap, root frames, retirement accounting —
  is owned by an explicit `Domain` value rather than by a thread-local.
- The thread-local retains a **cursor only**: a raw pointer to the domain and actor
  currently executing on this thread. It owns nothing and is re-established on
  every scheduler entry.
- Every cross-heap edge is explicit, enumerated and asserted, so step 4 has a
  defined set of references to sever rather than an open-ended audit.

### Non-goals

No parallelism. No second scheduler, no stealing, no migration, no preemption. The
scheduler remains FIFO and cooperative on one thread. Public Morrow semantics,
`extern "C"` signatures and the compiler's symbol contract are unchanged. Foreign
handles keep their current thread affinity.

## Design

### Ownership

```
Domain            (per scheduler; owns the invocation heap, frames, accounting)
  └── Actor       (owns its payload heap; Send)
        └── Heap  (roots, blocks, collections)
```

`Store` is split. The `slots` map, `next_heap`, `frames`, `next_frame` and
`retired_collections` fields become `Domain` state. The per-actor `Slot`/`Heap`
pair moves into the actor's own storage. The `active` cursor becomes a pointer to
the executing actor rather than an index into a map owned elsewhere.

`PhantomData<Rc<()>>` is removed from the heap types. `Heap` becomes `Send` and
stays `!Sync`: exactly one thread may touch a heap at a time, which the scheduler
guarantees by construction because an actor is only ever in one run queue.

### How a foreign function finds its heap

Two candidates were considered.

**Explicit context parameter.** Every `morrow_*` symbol takes `*mut Context` as its
first argument. No ambient state at all, but it changes all 257 ABI signatures,
the compiler's binding tables in `runtime.rs` and `runtime_abi.rs`, and the code
generated at every call site in both backends.

**Ambient current-context cursor (chosen).** The scheduler sets a thread-local raw
pointer to the executing actor's context before entering actor code and clears it
on exit. Foreign function signatures are unchanged. The thread-local no longer owns
heaps; it is a scheduling cursor, so an actor can be executed by any thread that
sets the cursor first. This is what BEAM does with its current-process pointer, and
it decouples this step from the codegen work in step 3.

The cost is that the ambient pointer must be correct at every entry and exit,
including panic and fault paths. That is enforced by a guard type that sets the
cursor on construction and clears it on drop, so the invariant holds on unwinding
and on the error paths that `docs/ACTOR_CLEANUP.md` already exercises.

### Control edges: the crux

An actor's PID holds a control edge into the invocation heap (`heaps.rs:86`). Under
migration those edges would span threads. This step does not sever them, but it
does make them the only such edges and proves it.

Every cross-heap reference is classified into exactly one of:

1. **Payload → payload across actors.** Must not exist. Messages are copied
   (`managed/copy.rs`), so this is already the invariant; step 1 adds an assertion
   that makes a violation a test failure rather than a latent migration bug.
2. **Payload → control in the invocation heap.** Exists today, enumerated here, and
   is the reference step 2 replaces with a global PID registry.
3. **Domain-internal.** Frames and retirement accounting; moves wholesale with the
   domain and never crosses actors.

A debug-mode verification pass walks every live heap and asserts that observed
edges fall only into those classes. It runs in the existing simulation scenarios,
where 965,734 scheduler turns already execute, giving the classification real
coverage rather than a hand-written sample.

### Message payloads stay copied

BEAM breaks its own isolation for binaries at or above 64 bytes, moving them to a
shared reference-counted off-heap area because copying large payloads on every send
is too expensive. That refcount is cross-thread shared mutable state.

Morrow copies every message payload, without exception, for the whole of this work.
Large-message cost stays a measured, documented limitation. Any future shared
payload representation is a separate decision requiring its own concurrency review;
it must not arrive as an optimization inside a scheduler change.

## Determinism and verification

Determinism moves from a property of the runtime to a property of the simulation
driver. This is forced by the model rather than chosen: Erlang guarantees signal
ordering only pairwise between two processes, never a global order across
schedulers, so bit-exact replay of a real multi-threaded run is not available in a
BEAM-shaped runtime. TigerBeetle and FoundationDB keep their cores single-threaded
precisely to retain it.

`crates/morrow-runtime/src/managed/simulation.rs` already provides "opt-in
deterministic control of the real native managed actor runtime" with a virtual
clock, and drives production code rather than a model. That is the pattern to
extend in step 2: one scheduler implementation with two drivers, where every
nondeterminism source — which scheduler runs next, the clock, steal and migration
choices, collection timing — sits behind the same interface the virtual clock uses
today. Production plugs in OS threads and real time; simulation plugs in a seeded
driver running N virtual schedulers on one real thread and can select adversarial
interleavings deliberately.

For step 1 specifically the gates are:

1. The full existing gate passes unchanged: `cargo xtask check`, the native output
   fixtures, examples, compatibility programs, atomic rejections and fuzz cases.
2. The recorded deterministic replay evidence reproduces exactly. Step 1 introduces
   no scheduling change, so any divergence is a defect in this step, which makes
   the existing scenario the sharpest available oracle.
3. The cross-heap edge classification pass reports only permitted classes across
   the seeded scenarios.
4. ThreadSanitizer runs clean over the runtime test suite. It finds nothing
   meaningful while execution is single-threaded; it is introduced here so the
   harness, suppressions and CI wiring exist before step 2 makes it load-bearing.
5. Allocation throughput and collection pause measurements are recorded before and
   after, against `benchmarks/language-comparison/`. Replacing a thread-local map
   lookup with pointer indirection should not regress the measured workloads; the
   claim needs numbers rather than assertion.

TSan and the seeded replay divide the space: replay covers logical scheduling,
sanitizers cover the physical layer of atomics and memory ordering that simulation
cannot model. From step 2 onward the second half is what replaces the bugs seeded
replay used to catch.

## Risks

**The ambient cursor is a correctness hazard.** A missed clear leaves a dangling
pointer to a retired actor. Mitigated by the RAII guard and by debug assertions
that the cursor is null on scheduler entry and non-null inside actor code.

**Conservative stack scanning interacts with ownership.** `README.md` records that
conservative scanning remains while precise-root coverage is completed. Per-thread
stacks are scanned by their own scheduler, which is compatible with this design,
but the precise-root work and this change touch the same code and should not be
interleaved in one commit.

**Slot 0 may hold more than control objects.** The audit in this step is what
establishes the true contents of the invocation heap. If it holds state that cannot
be classified into the three permitted edge classes, step 2's PID registry design
changes, and that finding should surface here rather than during step 4.

## Decisions to record

Decision156: heap ownership moves to actors and domains; the thread-local becomes a
scheduling cursor; message payloads remain copied without exception; determinism
becomes a property of the simulation driver rather than of the runtime.
