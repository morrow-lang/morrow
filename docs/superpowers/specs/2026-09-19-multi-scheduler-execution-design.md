# Multi-scheduler execution

Status: proposed. Date: 2026-09-19. Step 2 of 5 toward parallel actor execution.

Step 1 is [sendable actor heaps](2026-09-15-sendable-actor-heaps-design.md),
adopted as Decision156. It moved heap ownership out of thread-local storage into
an explicit `Domain` and left the thread-local as a cursor. It produced no
parallelism, and the `Activation` guard it introduced is still dead code:
`crates/morrow-runtime/src/memory/heaps.rs:111` and `:376` carry
`#[allow(dead_code)]` with a comment naming this step as the caller.

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

Verified at commit `6ea651e`. Five findings change the shape of this step relative
to the sketch in the step 1 spec.

### 1. The session lives inside the domain it would own

`crates/morrow-runtime/src/managed/lifecycle.rs:52` enters the invocation heap and
allocates the `Session` there:

```rust
let _control_scope = memory::enter_heap(0);
let s = allocate::<Session>();
```

Every `Actor` record, every `Pid` and the identity table are allocated the same
way. So `Session { domain: Domain }` is not expressible: the session is a managed
object inside slot 0 of the domain that would own it. The domain has to be
created, activated and dropped by an owner that sits **above** `morrow_managed_new`
and outlives `morrow_managed_stop`.

That owner does not exist today. `morrow_managed_new` is called directly by
compiled `main` and by `morrow_library_open`, and the only thing that ends an
invocation is `morrow_managed_close` (`managed/host.rs:48`), which is a host-side
entry point keyed by a `HOSTS` map rather than an owned runtime value.

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

1. **Domain identity on roots and scopes.** `Domain` gains a unique id; `Root` and
   `Scope` record theirs; retiring either under a different domain is a hard error
   rather than silent corruption. Behaviour-preserving; independently testable with
   two domains on one thread. Finding 2.
2. **An owned invocation above the session.** A runtime-owned value creates a
   `Domain`, activates it, drives `morrow_managed_new`, and drops the domain after
   `morrow_managed_stop`. Two sessions on one thread stop sharing an invocation
   heap, which is what lets the simulation driver host N schedulers. Finding 1.
3. **Immutable actor identity header.** Split `Actor` so PID validation reads only
   published, never-rewritten fields. Behaviour-preserving single-threaded.
   Finding 3.
4. **Message fragments.** Give `managed/copy.rs` an explicit destination and route
   `send` through a fragment the receiver adopts. Finding 4.
5. **Budget policy.** Settle split-versus-atomic for the session counters and
   record it as a decision, because it moves an observable limit. Finding 5.
6. **Per-scheduler run queues and a scheduler set.** Only now does the FIFO list on
   `Session` split, with the simulation driver running N schedulers on one thread.
7. **OS threads.** The production driver. ThreadSanitizer becomes load-bearing here
   rather than quiet.

Tasks 1 to 4 change no observable behaviour and each has its own oracle. Task 5 is
a decision. Tasks 6 and 7 are where parallelism appears and where the seeded
scenarios stop being a bit-exact oracle, for the reason Decision156 records.

## Verification

Each behaviour-preserving task reproduces the seeded actor scenario's trace hash
`01a55a0046de5614` with its recorded 48,334 callbacks, 1,226 delivered, 3,774
timeouts, 2,501 restarts and 9,977 churn actors, and zero residue at cleanup. That
is the sharpest oracle available while nothing about scheduling changes, and any
divergence in tasks 1 to 4 is a defect in that task.

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
