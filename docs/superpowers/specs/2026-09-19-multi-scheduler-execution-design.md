# Multi-scheduler execution

Status: all seven tasks adopted and verified (Decisions158–160). Date: 2026-09-19.
Step 2 of 5 toward parallel actor execution.

Decision161 subsequently adds native continuation quanta, optional work stealing,
eligible actor migration and resource pinning. The seven-task scope below remains
the historical pinned-scheduler milestone; the current behavior is documented in
the [actor runtime contract](../../ACTOR_RUNTIME.md).

Step 1 is [sendable actor heaps](2026-09-15-sendable-actor-heaps-design.md),
adopted as Decision156. It moved heap ownership out of thread-local storage into
an explicit `Domain` and left the thread-local as a cursor. It produced no
parallelism. The activation API in `crates/morrow-runtime/src/memory/heaps.rs`
originally carried `#[allow(dead_code)]`; task 6's scheduler set will use it.

## Goal

Run actors on more than one scheduler. At the end of step 2:

- A scheduler owns one `Domain` and one run queue, and activates its domain around
  every continuation it executes.
- An actor is pinned to the scheduler that spawned it for its whole life. Nothing
  migrates; that is step 4.
- A PID names an actor on any scheduler, and `send` from one scheduler to another
  delivers without either thread touching the other's heap.
- The seeded simulation driver runs N schedulers on one real thread, so scheduling
  choices stay a property of the driver and the existing scenarios keep working.

### Non-goals

No preemption: a continuation still runs to its own return, so one long callback
still blocks its scheduler. That is step 3. No work stealing, no heap migration,
no change to the thread affinity of foreign handles. Message payloads remain
copied without exception, as Decision156 requires.

## What the tree forces

The following findings describe the tree at commit `74fd147`. Tasks 1 and 2 below
resolve the token and control-storage findings; the remaining findings constrain
the later tasks. They change the shape of this step relative to the step 1 sketch.

### 1. Control storage and ordinary program data share the invocation heap

`crates/morrow-runtime/src/managed/lifecycle.rs:52` enters the invocation heap and
allocates the `Session` there:

```rust
let _control_scope = memory::enter_heap(0);
let s = allocate::<Session>();
```

A census of every allocation site in `managed/` separates cleanly in two:

- **Invocation heap (slot 0):** `Session`, its `identities` table, `Actor`
  (`lifecycle.rs:135`, `host.rs:142`) and `Supervisor` (`supervision.rs:51`). Each
  enters heap 0 explicitly before allocating.
- **Payload heaps:** `Pid`, mailbox `Message`, cleanup `Scope` and `Deferred`, and
  copied frames. Each is a value belonging to one actor.

That boundary is better than the step 1 spec assumed, and it is small: four record
kinds. But slot 0 is not a control arena. It is also where compiled `main`
allocates every ordinary program value, because `active` is 0 whenever no actor
scope is entered. So the invocation heap cannot simply become per-scheduler: the
four control records have to be reachable from every scheduler, while program data
belongs to whichever domain allocated it.

This retires the framing this document first gave task 2 — "an owned invocation
above the session". Giving each invocation its own domain does not help, because
the sharing boundary is not the session, and because compiled code has no way to
name which invocation an ordinary allocation belongs to. What has to move is the
four control records, out of the collected heap entirely.

### 2. Roots and scopes are not domain-aware, so activation is unsafe today

`Root` records the heap slot it registered in (`heaps.rs`), and `remove_root` routes
through whichever domain is current:

```rust
pub(super) fn remove_root(heap: usize, id: usize) { /* ... */ }
pub(crate) fn remove_root(&mut self, heap: usize, id: usize) {
    if let Some(slot) = self.slots.get_mut(&heap) { slot.heap.roots.remove(&id); }
}
```

Slot ids and root ids are per-domain counters, both starting at 0 and 1. A `Root`
created under domain A and dropped while domain B is current therefore removes
*B's* root with the same numbers — silently, because `BTreeMap::remove` on a
missing key is a no-op and on a colliding key is a wrong unrooting. `Scope::drop`
has the same shape and reaches `leave`, whose `assert_eq!(self.active, entered)`
would fire against an unrelated domain's cursor.

This is not reachable in a shipped program, because nothing calls `activate` outside
the tests, and it is why step 1 could leave it alone. It is reachable the moment a
scheduler activates a domain. **Making `Root` and `Scope` name their domain is a
prerequisite for every other task in this step**, and it is the first thing to
implement.

### 3. A stale PID must still reach a retired actor record

The obvious design — a PID carries `(scheduler, slot, generation)` and resolves
through the owning scheduler's identity table — does not preserve current
behaviour. `managed/identity_tests.rs` requires that a PID whose actor has died and
whose slot has been **reused by a different actor** still resolves its supervision
lineage:

```rust
let result = morrow_managed_supervised_current(exec, stale.cast()) as *const abi::ResultValue;
assert_eq!((*result).tag, 0);
assert_eq!((*((*result).value as *const Pid)).actor, current);
```

`morrow_managed_supervised_current` reads `(*(*pid).actor).supervisor` and follows
it to the live replacement (`managed/supervision.rs:156`). The retired `Actor`
record is kept alive for exactly this purpose by the PID's control edge, and
Decision129 records stale-PID rejection and supervision lineage as guaranteed
behaviour. The identity table cannot answer this question, because the slot no
longer names that actor.

So the pointer stays. What has to change is that reading it across schedulers is
currently a data race: `valid_pid`/`live_pid` (`managed.rs:153` and `:166`)
dereference `exec.session`, `id`, `mailbox`, `alive` and `slot` on an actor its own
scheduler is concurrently mutating. The answer is to split the `Actor` record into
an **immutable identity header** — session/scheduler, id, slot, mailbox descriptor,
and the supervision anchor — published once and never rewritten, and a mutable
region only its owning scheduler touches. `alive` becomes an atomic in the header,
read as a hint and re-checked by the owner before delivery.

### 4. Send writes into the receiver's heap

`morrow_managed_send` (`managed/lifecycle.rs:195`) copies the message into the
target actor's heap:

```rust
let _receiver_scope = memory::enter_heap((*a).heap);
let copied = copy::value(s, ty, value);
```

Across schedulers that is a write into a heap another thread owns, and
`Domain::collect_active` may be sweeping it at that moment. Copy-at-send has to be
kept — deferring the copy to delivery would move allocation failure from the
sender's `result_err(4)` to a point where there is no caller to report it to — so
the copy needs a destination owned by neither heap.

This is BEAM's message fragment: the sender copies the term into a self-contained
off-heap block attached to the message, and the receiver adopts the block when it
dequeues. `managed/copy.rs` allocates exclusively through `memory::alloc` into the
entered heap, so it needs a destination abstraction before it can target a
fragment. Fragments are charged to the sender at send time, which keeps the
existing `charge`/`release` accounting exact.

### 5. Session-wide counters are the accounting unit

`Session` holds `live`, `messages`, `retained`, `next_id`, `used_slots`,
`identities`, `next_deadline` and `stopped`, and `charge`/`release` assert against
a single 64 MiB `BYTES` budget. These are consulted on every spawn and send. Per
scheduler they must either be split — each scheduler owning its share of the
budget — or made atomic. Splitting changes observable limits, because a program
that fits in one 64 MiB budget may not fit in N smaller ones; making them atomic
keeps the limit exact at the cost of contention on the send path. The limits are
observable through `result_err(4)`, so this is a behaviour decision, not an
implementation detail, and it should be settled before the queues are split.

## Ordered tasks

1. **Domain identity on roots and scopes.** *Adopted as Decision158.* `Domain`
   gains a unique id; `Root` and `Scope` record theirs; retiring either under a
   different domain is a hard error rather than silent corruption. Creating a
   payload heap whose control record does not live in the invocation heap is
   rejected for the same reason. Behaviour-preserving. Finding 2.
2. **Lift control storage out of the collected invocation heap.** *Adopted as
   Decision159.* Move `Session`,
   the identity table, `Actor` and `Supervisor` to explicitly reference-counted
   allocations released when the last reference dies, so a control record is
   reachable from any scheduler and no longer swept by one scheduler's collector.
   PID wrappers retain controls explicitly, so task 4's message fragments can
   transfer those references without exposing collected storage. Findings 1 and 3.
3. **Immutable actor identity header.** *Implemented in Decision160.* Split `Actor` so PID validation reads only
   published, never-rewritten fields, and the retired record a stale PID follows
   stays readable. Behaviour-preserving single-threaded. Finding 3.
4. **Message fragments.** *Implemented in Decision160.* Give `managed/copy.rs` an explicit destination and route
   `send` through a fragment the receiver adopts. Finding 4.
5. **Budget policy.** *Decision160 preserves one exact atomic invocation budget.* Settle split-versus-atomic for the session counters and
   record it as a decision, because it moves an observable limit. Finding 5.
6. **Per-scheduler run queues and a scheduler set.** *Implemented in Decision160.* Only now does the FIFO list on
   `Session` split, with the simulation driver running N schedulers on one thread.
7. **OS threads.** *Implemented in Decision160, opt-in through `MORROW_SCHEDULERS`.* The production driver. ThreadSanitizer becomes load-bearing here
   rather than quiet.

Tasks 1 to 4 change no observable behaviour and each has its own oracle. Task 5 is
a decision. Tasks 6 and 7 are where parallelism appears and where the seeded
scenarios stop being a bit-exact oracle, for the reason Decision156 records.

## Verification

Each behaviour-preserving task reproduces an actor scenario under the same seed
and step count before and after the change. The task 2 baseline uses
`cargo run -p morrow-sim -- --actors --seed 0x0046524e --steps 5000 --json`:
trace hash `0xd4e402a412f11e2f`, 48,739 callbacks, 1,246 delivered, 3,754 timeouts,
2,507 restarts, 10,000 churn actors and zero residue at cleanup. Decision156's
`01a55a0046de5614` remains historical evidence for its recorded run, not a substitute
for a freshly matched configuration. Any scheduling divergence in tasks 1 to 4
is a defect in that task. Physical memory totals include external control storage;
independent weak-reference probes additionally verify exact final reclamation.

From task 6 the trace hash is no longer expected to be stable across scheduler
counts. What replaces it: the driver replays a *recorded interleaving* rather than
a recorded outcome, the cross-heap edge classification from step 1 runs unchanged,
and ThreadSanitizer moves from quiet to load-bearing.

## Risks

**The budget split is a user-visible limit change.** Finding 5. Deciding it late,
after the queues are split, means discovering it as a test failure in a program
that used to fit.

**The identity header split touches the supervision lineage.** Finding 3's
requirement is enforced by one assertion in `identity_tests.rs`; the split must not
be allowed to relax it, and the header has to carry enough to answer the lineage
question without reading mutable state.

**Nothing here gives preemption.** A single long callback still blocks its
scheduler, so the first parallel measurements will look worse than they should on
any workload with uneven callbacks. That is step 3, and the measurement write-up
has to say so rather than presenting step 2 numbers as the ceiling.

## Implemented execution contract

The production count defaults to one and accepts `MORROW_SCHEDULERS=1..64`.
The native host can configure before actor publication and explicitly place root
spawns; ordinary root spawns use round-robin placement. Actor children and
supervision stay on their spawning scheduler. Each OS worker owns a Domain and
fault cell. Stops, including partial startup failures, join workers before the
host releases descriptors. Queue publication rejects copies completed after
stop, and quiescence includes active callbacks and pending transport.

The feature-gated `managed::simulate_schedulers` and `scheduler_recording` APIs
run/replay scheduler choices through those same owner operations on one thread.
The shared limits count fragments before adoption and are independent of scheduler
count. See [the current runtime contract](../../ACTOR_RUNTIME.md) for APIs, ordering
and remaining limitations. The findings above retain the preimplementation
census; they do not describe today's control allocation or send path.
