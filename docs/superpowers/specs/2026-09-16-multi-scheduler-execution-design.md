# Multi-scheduler execution

Step 2 of five toward parallel actors. Step 1 moved heap ownership into an
explicit `Domain` and left the thread-local as a cursor
([design](2026-09-15-sendable-actor-heaps-design.md), Decision156). This step
spends that ownership: N schedulers run actors on N threads at once.

## Why

The runtime executes every actor on one thread. `ROADMAP.md` records actor
throughput and scheduler fairness as unmeasured against BEAM, and
`benchmarks/language-comparison/BEAM.md` states outright that the comparison
"establishes no actor, scheduler or distributed-system performance parity".
Neither claim can be closed without running actors in parallel.

Step 1 was judged on whether it made this step possible without a second
ownership rewrite. This document is where that judgement is tested.

## Current architecture

Verified in the tree at commit `4daffcf`:

- `crates/morrow-runtime/src/managed.rs:84` `Session` fuses three concerns: the
  FIFO run queue (`first`, `last`), the identity registry (`identities`,
  `next_id`, `used_slots`) and invocation accounting (`live`, `messages`,
  `retained`, `stopped`, `next_deadline`).
- `managed.rs:77` `Pid` holds `*mut Session` and `*mut Actor` as raw pointers.
  `valid_pid` (`managed.rs:153`) and `live_pid` (`managed.rs:166`) validate
  identity by dereferencing that actor pointer.
- `managed.rs:138` `new_pid` registers a `control_edge` from the PID wrapper into
  invocation heap slot 0.
- `managed/lifecycle.rs:195` `morrow_managed_send` enters **the receiver's heap**,
  copies the payload there with `copy::value`, links the message into the
  receiver's mailbox, increments `(*a).messages` and `(*s).messages`, and may
  `enqueue` the receiver.
- `managed/scheduler.rs:142` `idle` decides deadlock locally: an empty queue with
  no pending deadline raises fault 10.
- `managed/supervision.rs:136` records that restart issues a **fresh** identity:
  "Old PIDs remain invalid for send; lookup never redirects an existing PID."
- `managed/host.rs:10` `HOSTS` is a thread-local root map; `morrow_managed_poll`
  (`host.rs:64`) is the embedded host's non-blocking driver.
- `crates/morrow-runtime/src/memory.rs:62` `Heap` is a `BTreeMap<usize, Block>`
  over individually allocated blocks, not a bump or copying heap.
- `memory/heaps.rs:59` `verify_edges` asserts exactly three permitted edge
  classes and is exercised by a deliberately failing negative case.

Two facts follow, and they set the whole design.

**A `Heap` is a map over stable addresses.** Moving an object graph between heaps
is a map merge, not a relocation: no pointer rewriting, no second copy. BEAM pays
real cost for off-heap message fragments; here they are nearly free. This makes
fragment transfer the cheapest correct send mechanism rather than the most
expensive one.

**Identity already survives restart correctly.** Because a restarted actor gets a
new id and old PIDs stay invalid, a message in flight to a crashing actor is
addressed to a dead generation. It is dropped, with no rendezvous between the
crashing scheduler and the sending one. The "let it crash" semantics Morrow
already implements resolve the hardest-looking cross-thread race for free.

## Goal

N schedulers execute actor continuations on N threads simultaneously, with
measured scaling, and with every step 1 oracle still green.

Concretely:

- Each scheduler owns a `Domain` and a run queue; a running callback touches no
  shared mutable state.
- A sender never dereferences an `Actor`. Cross-scheduler send carries its payload
  in a detached heap fragment the receiver adopts on its own thread.
- Actors are placed on a scheduler at spawn and pinned there for life. No
  migration, no stealing.
- Deterministic replay extends to N>1 through a seeded driver over logical
  schedulers.

### Non-goals

No preemption: the scheduler stays cooperative, and a callback still runs to
completion (step 3). No work stealing or heap migration (step 4). No change to
foreign handle thread affinity (step 5). Public Morrow semantics and the
compiler's symbol contract are unchanged.

## Design

### Three ownership tiers

`Scheduler`, one per thread, owning a `Domain`: run queue, `next_deadline`,
logical clock, and the actors pinned to it. The only tier a running callback
touches, and it touches it without synchronization. This is what protects the
allocation hot path, where step 1's measured 1.58% regression already lives.

`Registry`, shared: the `id -> (scheduler, slot, generation)` table that
`identities` is today, sharded by id, plus ID issuance. `valid_pid` and `live_pid`
become registry lookups. They get stronger rather than weaker: generation
checking stops depending on an actor pointer remaining dereferenceable.

`Invocation`, shared: `live`, `messages`, `retained`, `stopped` and the root fault
cell, as atomics. `charge`/`release` (`managed.rs:185`, `:195`) become
compare-exchange loops so the `BYTES`, `MESSAGES` and `LIVE` limits stay global and
exact. A program that fails on the 64 MB ceiling today fails identically at N=8.

### `Pid` loses its pointers

`{session, actor, id, mailbox}` becomes `{id, generation, mailbox}`.

This is the change that keeps step 1's edge oracle honest. With no `*mut Actor`
inside a PID there is no representable cross-domain pointer, so `verify_edges`
asserts the same three edge classes without modification. The `control_edge` into
slot 0 remains: a PID still roots its own control word inside its own domain.

### Send and adoption

The registry hands a sender an `Arc<Inbox>` — mailbox type, atomic message count,
outer queue, liveness generation — which outlives its actor. `Actor` stays
domain-local, touched only by its owning scheduler. There is no shared mutable
actor state for a sender to reach.

`morrow_managed_send` becomes: resolve through the registry; reject on generation
or mailbox-type mismatch with `abi::result_err(3)` exactly as today; check
per-inbox and global caps; run the `cost::value` preflight unchanged; then copy
into a **detached fragment** — a `Heap` owned by the `Message` — instead of
entering the receiver's heap. `managed/copy.rs` is reused unmodified: it already
copies into whatever heap is active.

Mailboxes use BEAM's two-queue split. Senders push onto an atomic outer queue; the
owning scheduler splices the whole outer queue into a private inner queue in one
exchange and works from it with no synchronization. Per-sender ordering is
preserved because a sender's pushes are sequential and the splice reverses
uniformly. `managed/receive.rs` selectors and timeouts operate on the inner queue.

Adoption merges the fragment's `blocks` into the actor's heap and adds its `bytes`.
Stable addresses mean no rewriting and no re-copy. Before adoption a fragment is
unreachable from any heap, which is safe because fragments are allocate-only and
never collected; they exist for one in-flight message.

Cross-scheduler wakeups use the same shape: a remote run queue other threads push
to and the owner splices at loop top, plus a condvar to wake a parked scheduler.

### Placement at spawn

Step 1's design says actors are "pinned to their spawning scheduler". Taken
literally that yields no parallelism: programs spawn their actors from main, which
runs on scheduler 0, so every actor would land there and the scaling benchmark
would measure thread creation. Placement and pinning are therefore separate.
`morrow_managed_spawn` **places** a new actor by policy — round-robin over
schedulers, which is sufficient without migration and avoids reading other
schedulers' load — and the actor is pinned to that scheduler for life.

Remote placement reuses the send machinery exactly. The spawning thread copies the
initializer frame into a detached fragment and pushes a spawn request onto the
target scheduler's remote queue; the target allocates the `Actor` in its own
domain and adopts the fragment. The spawning thread never touches the target's
domain, so `lifecycle.rs:134` and `:146` — which today enter heap 0 and then the
child's heap — become local operations on whichever scheduler materializes the
actor.

`morrow_managed_spawn` must still return a usable PID synchronously. It can: the
registry issues the id, generation and `Arc<Inbox>` before the target materializes
the `Actor`. Sending to an actor that has not yet been materialized is well
defined, because the inbox exists and the messages are adopted in order after the
spawn request.

Supervised children are the exception and are **always co-located with their
supervisor**. `(*a).supervisor` (`managed.rs:71`) points at a `Supervisor` owned by
the supervising domain; placing a child elsewhere would create exactly the
cross-domain pointer `verify_edges` forbids. Co-location is what keeps restart
local and lock-free, and it is now an enforced rule rather than an accident of
where `spawn` was called.

### Termination detection

This is the genuinely new problem. `idle` (`scheduler.rs:142`) currently infers
deadlock from local emptiness and raises fault 10. Under N schedulers a scheduler
with nothing to do may be waiting on a message another thread is mid-send, so
local emptiness no longer implies global termination.

The invocation carries an atomic pair: runnable actors, and in-flight messages —
where a message is in flight from `charge` until adoption. Only the last scheduler
to park evaluates the global condition. Getting this wrong yields either a hang or
a spurious fault 10, so it carries its own oracle rather than riding along on the
others.

### Supervision, faults and stop

Supervision lineages are co-located by the placement rule above, so restart is
entirely local and lock-free. Only escalation — a lineage exhausting its 32
restarts — crosses threads, through the shared `stopped` flag.

"Let it crash" covers actor-level failure and nothing more. A lost message, a
doubly adopted fragment or torn accounting is not something a restart can retry;
restarting an actor onto a corrupted heap propagates the corruption. BEAM draws the
same line, supervising process errors and treating emulator invariant failure as
fatal. The principle makes the actor-crash paths cheap; it justifies no reduction
in the synchronization the registry, fragments and counters require.

A root fault sets `stopped`; every scheduler observes it at loop top and unwinds
its own actors through the existing `cleanup::unwind`/`finish` path.
`morrow_managed_run` returns only after all schedulers quiesce, preserving its
current contract.

### Startup and configuration

The compiler-emitted ABI does not change. `morrow_managed_run` keeps its
signature: the calling thread becomes scheduler 0, spawns N-1 workers and joins
them before returning.

Scheduler count is a deployment property, not program semantics — BEAM keeps it as
an emulator flag rather than language surface, so the same artifact runs on 1 core
or 96 without recompiling. Embedders (morrow-web, morrow-cluster, tests,
benchmarks) configure it through a plain Rust options value on `morrow-runtime`,
with no new `no_mangle` symbol. Deployed binaries configure it through an
environment variable, defaulting to `available_parallelism()` natively and to 1
where threads are unavailable. `crates/morrow-web/src/lib.rs:43` already caps its
own pool at `min(4)`, so an embedder-facing knob is required, not speculative.

Host mode pins to one scheduler: an invocation opened through
`morrow_managed_open` runs at N=1. `poll`'s "advance ready continuations up to
`max_steps`" contract is then unchanged, `HOSTS` stays correct as a thread-local,
and morrow-web keeps its current behavior. Driving N schedulers cooperatively from
a host poll loop is a materially larger contract and is out of scope here.

## Determinism and verification

Determinism is a property of the simulation driver, not of the runtime
(Decision156). Under the `simulation` feature, N schedulers become N cooperative
state machines advanced by a seeded driver on one thread, with a decision point at
each place real threads would interleave: outer-queue splice, remote run-queue
splice, registry lookup, park, unpark. Every cross-scheduler operation funnels
through those five points by construction, so the driver covers the interleaving
surface without instrumenting the hot path and costs nothing in production builds.

That yields exact replay at N>1: a seed plus a trace hash, the same oracle shape as
the `01a55a0046de5614` hash step 1 records, now parameterized by scheduler count. A
cross-scheduler ordering bug becomes a reproducible hash mismatch rather than an
intermittent report.

Oracles, in the order they would catch a defect:

1. Step 1's oracles, unchanged and still green: trace hash at N=1, `verify_edges`
   with its failing negative case, zero cleanup residue.
2. Seeded replay at N=2, 4, 8: same seed, same hash, across repeated runs.
3. Per-pair message ordering, and no loss or duplication, over seeded runs.
4. Accounting conservation: `retained` and `messages` return to zero at quiesce
   under every seed.
5. Termination detection: seeds shaped so schedulers park while a message is in
   flight, asserting neither a hang nor a spurious fault 10.
6. Fragment adoption: every block accounted for exactly once, no fragment
   outliving adoption.
7. ThreadSanitizer over the real-threaded suite, extending the gate added in
   `8a8798c`.

## Evidence

A CPU-bound multi-actor workload measured as a scaling curve at 1, 2, 4 and 8
schedulers, with interleaved samples in the style of
`benchmarks/language-comparison/NATIVE_OPTIMIZATION.md`, plus the Elixir
equivalent with `+S` varied.

The writeup states the preemption asymmetry prominently: Morrow's scheduler is
cooperative until step 3 and BEAM's is preemptive, so BEAM should win on
fairness-sensitive shapes. The comparison is about scaling behavior, not a
throughput verdict, and claiming otherwise would repeat the error `BEAM.md`
already guards against.

Step 1's 1.58% allocation regression is re-measured here rather than inherited.
Making the cursor path hot was the stated reason to expect recovery; this is where
that expectation is checked.

## Risks

**Termination detection is the likeliest source of a hang.** It is the one piece
with no single-threaded analogue to fall back on. Oracle 5 exists for it
specifically.

**Atomic accounting is a contention point on the send path.** Global exactness of
`BYTES`/`MESSAGES` is a behavioral contract, so it cannot be relaxed to per-scheduler
approximation without changing observable failure. If the scaling curve flattens,
this is the first place to measure.

**The scaling benchmark can flatter itself.** A workload with no cross-scheduler
sends measures thread spawning, not the design. The workload must exercise real
cross-scheduler traffic, and the writeup must say what fraction of sends crossed.

**Supervised co-location can defeat round-robin placement.** A program whose actors
are mostly supervised children of one parent piles its whole tree onto one
scheduler, because co-location overrides placement. Without migration there is no
remedy inside this step; the benchmark must report the resulting distribution so
the limitation is visible rather than hidden in an average.

**Cooperative scheduling bounds achievable scaling.** Without preemption a long
callback blocks its scheduler, so fairness-sensitive shapes will scale poorly until
step 3. Expected, and to be recorded rather than designed around.

## Decisions to record

- Heap ownership per scheduler; `Pid` becomes an opaque `{id, generation, mailbox}`
  resolved through a shared registry; no pointer crosses a domain.
- Cross-scheduler send transfers a detached heap fragment, adopted by the receiver
  on its own thread as a map merge.
- Actors are placed round-robin at spawn and pinned for life; supervised children
  are co-located with their supervisor, which keeps restart local and keeps the
  supervisor pointer inside one domain. This deviates from step 1's "pinned to
  their spawning scheduler", which would have produced no parallelism.
- Scheduler count is runtime configuration, not language surface; the
  compiler-emitted ABI is unchanged.
- Host mode (`open`/`poll`/`close`) runs at N=1.
- Deterministic replay extends to N>1 through a seeded logical-scheduler driver.
