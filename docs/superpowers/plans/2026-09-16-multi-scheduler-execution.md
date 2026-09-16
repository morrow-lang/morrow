# Multi-Scheduler Execution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run actor continuations on N scheduler threads simultaneously, with measured scaling and every step 1 oracle still green.

**Architecture:** Each scheduler owns a `Domain` and a run queue and touches no shared mutable state on the hot path. Identity moves out of `Session` into a shared `Registry` that hands senders an `Arc<Inbox>`, so a sender never dereferences an `Actor`. Cross-scheduler send and spawn carry their payload in a detached heap fragment that the receiving scheduler adopts on its own thread as a `BTreeMap` merge over stable addresses.

**Tech Stack:** Rust 2024 (`rust-toolchain.toml`), `morrow-runtime` crate, Cranelift-backed native tests, `cargo xtask` gate, ThreadSanitizer, Elixir/OTP for the comparison workload.

**Spec:** `docs/superpowers/specs/2026-09-16-multi-scheduler-execution-design.md`

## Global Constraints

- The compiler-emitted ABI does not change. No `#[unsafe(no_mangle)]` signature in `crates/morrow-runtime/src/managed*` may change, and `crates/morrow/src/runtime.rs` and `crates/morrow/src/runtime_abi.rs` must not be edited by this plan.
- Global limits stay exact, not per-scheduler approximations: `LIVE = 1024`, `IDS = 65536`, `MAILBOX = 4096`, `MESSAGES = 65536`, `BYTES = 64 * 1024 * 1024` (`crates/morrow-runtime/src/managed.rs:9-13`).
- Step 1's oracles must stay green at every commit: the seeded scenario trace hash at one scheduler, `Domain::verify_edges` with its deliberately failing negative case, and zero cleanup residue.
- `Domain: Send` must continue to hold; the compile-time guard at `crates/morrow-runtime/src/memory/heaps.rs:485` stays.
- Actors are placed at spawn and pinned for life. No migration and no work stealing in this plan; those are step 4.
- Supervised children are always co-located with their supervisor.
- Host mode (`morrow_managed_open` / `morrow_managed_poll` / `morrow_managed_close`) runs at exactly one scheduler.
- Scheduler count is runtime configuration. It must never appear in Morrow source or the compiler symbol contract.
- Do not weaken a failing oracle to make a gate pass. If a test relies on timing, make its setup deterministic while preserving the behavioral assertion.

## File Structure

**New files**

- `crates/morrow-runtime/src/managed/registry.rs` — `Registry`, `Inbox`, id issuance, generation validation, `Arc<Inbox>` handout. Owns everything a sender may touch.
- `crates/morrow-runtime/src/managed/placement.rs` — round-robin placement policy and the supervised co-location rule.
- `crates/morrow-runtime/src/managed/quiesce.rs` — the runnable/in-flight atomic pair and global termination detection.
- `crates/morrow-runtime/src/managed/threads.rs` — scheduler thread spawning, parking, joining, and the configuration resolution.
- `crates/morrow-runtime/src/managed/registry_tests.rs`, `quiesce_tests.rs` — unit oracles for the two pieces with no single-threaded analogue. `Fragment` lives in the memory layer beside `Heap`, so its oracles go in the existing `crates/morrow-runtime/src/memory/tests.rs`.
- `benchmarks/language-comparison/programs/actor_scaling.mr` and its Elixir counterpart — the scaling workload.
- `benchmarks/language-comparison/SCHEDULERS.md` — the scaling and BEAM comparison evidence.

**Modified files**

- `crates/morrow-runtime/src/managed.rs` — `Session` splits into `Scheduler` + shared state; `Pid` loses its pointers; `charge`/`release` become atomic.
- `crates/morrow-runtime/src/managed/lifecycle.rs` — `morrow_managed_send` and `morrow_managed_spawn` route through registry, fragment and placement.
- `crates/morrow-runtime/src/managed/scheduler.rs` — per-scheduler run queue, remote queue splice, `idle` replaced by global quiescence.
- `crates/morrow-runtime/src/managed/supervision.rs` — co-location rule on restart.
- `crates/morrow-runtime/src/memory/heaps.rs` — fragment slots, `adopt`, fragment exclusion from collection and edge verification.
- `crates/morrow-runtime/src/managed/simulation/scenario.rs` — `Config` gains `schedulers`; the driver interleaves logical schedulers.
- `xtask/src/main.rs` — TSan over the threaded suite, benchmark staging.
- `DECISIONS.md`, `ROADMAP.md`, `docs/ACTOR_RUNTIME.md` — the record.

## Sequencing

Tasks 1-3 build identity and the inbox with no behavior change and no threads. Tasks 4-5 move message payloads into fragments, still single-threaded — after Task 5 the send path no longer touches the receiver's heap, which is the ownership crux. Tasks 6-8 split the scheduler and add termination detection. Task 9-10 add the deterministic multi-scheduler driver and its invariant oracles, still on one OS thread: every parallel semantic is exercised reproducibly before a single thread is spawned. Tasks 11-12 spawn real threads and add TSan. Tasks 13-15 produce the evidence and the record.

The ordering is deliberate: a defect in identity, fragments or quiescence surfaces as a reproducible seeded failure in Tasks 1-10, so Task 11's failures are threading failures and nothing else.

---

### Task 1: Introduce the identity `Registry`

Today identity lives in three `Session` fields — `identities`, `next_id`, `used_slots` — and validation dereferences an actor pointer (`managed.rs:153`). This task builds the replacement as a standalone, testable value. Nothing is wired to it yet, so it can be reviewed on its own.

The design keeps today's exact semantics: a reusable slot plus a monotonic generation. Generation is what `Session.next_id` is today, renamed because with opaque PIDs it is the only thing distinguishing an old identity from the actor now occupying its slot.

**Files:**
- Create: `crates/morrow-runtime/src/managed/registry.rs`
- Create: `crates/morrow-runtime/src/managed/registry_tests.rs`
- Modify: `crates/morrow-runtime/src/managed.rs` (module declarations near `:5-8`)

**Interfaces:**
- Produces: `Registry::new()`, `Registry::issue(scheduler: usize) -> Option<Handle>`, `Registry::resolve(&self, slot: usize, generation: u64) -> Option<&Entry>`, `Registry::retire(&mut self, slot: usize)`, `Registry::live(&self) -> usize`, `struct Handle { slot: usize, generation: u64 }`, `struct Entry { generation: u64, scheduler: usize }`.

- [ ] **Step 1: Write the failing test**

Create `crates/morrow-runtime/src/managed/registry_tests.rs`:

```rust
use super::registry::Registry;

#[test]
fn slot_reuse_issues_a_fresh_generation_that_invalidates_the_old_identity() {
    let mut registry = Registry::new();
    let first = registry.issue(0).expect("capacity");
    assert_eq!(first.slot, 0);
    assert_eq!(first.generation, 1, "generations start at 1 so 0 stays invalid");

    registry.retire(first.slot);
    assert!(
        registry.resolve(first.slot, first.generation).is_none(),
        "a retired identity must not resolve"
    );

    let second = registry.issue(0).expect("capacity");
    assert_eq!(second.slot, first.slot, "the vacant slot is reused");
    assert!(
        second.generation > first.generation,
        "reuse must not resurrect the old identity"
    );
    assert!(
        registry.resolve(first.slot, first.generation).is_none(),
        "the old generation stays invalid after the slot is reoccupied"
    );
    assert_eq!(
        registry.resolve(second.slot, second.generation).map(|e| e.scheduler),
        Some(0)
    );
}

#[test]
fn placement_scheduler_is_recorded_per_identity() {
    let mut registry = Registry::new();
    let a = registry.issue(0).expect("capacity");
    let b = registry.issue(3).expect("capacity");
    assert_eq!(registry.resolve(a.slot, a.generation).unwrap().scheduler, 0);
    assert_eq!(registry.resolve(b.slot, b.generation).unwrap().scheduler, 3);
    assert_eq!(registry.live(), 2);
}

#[test]
fn exhausted_generations_refuse_to_issue_rather_than_wrapping() {
    let mut registry = Registry::new();
    registry.force_generation_for_test(u64::MAX);
    assert!(
        registry.issue(0).is_none(),
        "generation exhaustion must refuse, matching the old next_id admission check"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime registry`
Expected: FAIL to compile — `registry` module does not exist.

- [ ] **Step 3: Write the registry**

Create `crates/morrow-runtime/src/managed/registry.rs`:

```rust
//! Identity independent of actor storage: a reusable slot plus a monotonic
//! generation. Validation never dereferences an actor, so a PID stays meaningful
//! after its actor is freed and after another actor occupies its slot.
use super::IDS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Handle {
    pub slot: usize,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Entry {
    pub generation: u64,
    pub scheduler: usize,
}

pub(super) struct Registry {
    entries: Vec<Option<Entry>>,
    next_generation: u64,
}

impl Registry {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_generation: 0,
        }
    }

    /// Reuse vacant storage without resurrecting the identity that held it.
    pub(super) fn issue(&mut self, scheduler: usize) -> Option<Handle> {
        let generation = self.next_generation.checked_add(1)?;
        let slot = match self.entries.iter().position(Option::is_none) {
            Some(slot) => slot,
            None if self.entries.len() < IDS => {
                self.entries.push(None);
                self.entries.len() - 1
            }
            None => return None,
        };
        self.next_generation = generation;
        self.entries[slot] = Some(Entry {
            generation,
            scheduler,
        });
        Some(Handle { slot, generation })
    }

    pub(super) fn resolve(&self, slot: usize, generation: u64) -> Option<&Entry> {
        let entry = self.entries.get(slot)?.as_ref()?;
        (entry.generation == generation && generation != 0).then_some(entry)
    }

    pub(super) fn retire(&mut self, slot: usize) {
        if let Some(entry) = self.entries.get_mut(slot) {
            *entry = None;
        }
    }

    pub(super) fn live(&self) -> usize {
        self.entries.iter().filter(|e| e.is_some()).count()
    }

    #[cfg(test)]
    pub(super) fn force_generation_for_test(&mut self, generation: u64) {
        self.next_generation = generation;
    }
}
```

Declare both modules in `crates/morrow-runtime/src/managed.rs`, beside the existing `copy`/`cost` declarations:

```rust
#[path = "managed/registry.rs"]
mod registry;
#[cfg(test)]
#[path = "managed/registry_tests.rs"]
mod registry_tests;
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p morrow-runtime registry`
Expected: PASS, three tests.

- [ ] **Step 5: Run the crate suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS. Nothing is wired yet, so every existing test must be untouched.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/managed/registry.rs \
        crates/morrow-runtime/src/managed/registry_tests.rs \
        crates/morrow-runtime/src/managed.rs
git commit -m "feat(runtime): add an identity registry independent of actor storage"
```

---

### Task 2: `Pid` loses its pointers

This is the change that keeps step 1's edge oracle meaningful: with no `*mut Actor` inside a PID, no cross-domain pointer is representable, and `Domain::verify_edges` (`memory/heaps.rs:59`) keeps asserting the same three edge classes without modification.

`Pid` is `#[repr(C)]` and reachable from generated code only as an opaque pointer, so its layout may change freely — but it is allocated in the invocation heap and scanned conservatively, so the test must assert that it no longer contains anything the collector would follow as an actor edge.

**Files:**
- Modify: `crates/morrow-runtime/src/managed.rs:77-84` (`Pid`), `:138-148` (`new_pid`), `:153-174` (`valid_pid`, `live_pid`), `:84-100` (`Session` gains `registry`)
- Modify: `crates/morrow-runtime/src/managed/lifecycle.rs` (call sites of `(*pid).actor`)
- Modify: `crates/morrow-runtime/src/managed/supervision.rs:136-160` (`morrow_managed_supervised_current`)
- Test: `crates/morrow-runtime/src/managed/tests.rs`

**Interfaces:**
- Consumes: `Registry`, `Handle`, `Entry` from Task 1.
- Produces: `Pid { slot: usize, generation: u64, mailbox: *const Type }`; `unsafe fn actor_of(s: *mut Session, pid: *const Pid) -> *mut Actor` as the single place a PID is turned into an actor pointer, valid only on the owning scheduler.

- [ ] **Step 1: Write the failing test**

Add to `crates/morrow-runtime/src/managed/tests.rs`:

```rust
#[test]
fn pid_carries_identity_only_and_never_an_actor_pointer() {
    assert_eq!(
        std::mem::size_of::<Pid>(),
        std::mem::size_of::<usize>() * 3,
        "slot, generation and mailbox descriptor — no session or actor pointer"
    );
}

#[test]
fn a_pid_outlives_its_actor_and_reports_the_death_instead_of_dereferencing() {
    // A PID held across its actor's death must fail validation by generation,
    // not by reading freed actor storage. Under the old representation this
    // test could only pass by luck; the registry makes it a property.
    let outcome = simulation::run(simulation::Config {
        seed: 0x5eed_0002,
        steps: 4096,
        schedulers: 1,
    })
    .expect("scenario completes");
    assert_eq!(outcome.final_live, 0);
    assert_eq!(outcome.final_messages, 0);
    assert!(
        outcome.churn > 0,
        "the scenario must actually retire and respawn actors for this to mean anything"
    );
}
```

Note: `schedulers` is added to `Config` in Task 9. Until then write `simulation::Config { seed: 0x5eed_0002, steps: 4096 }` and add the field to this call in Task 9.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime pid_carries_identity_only`
Expected: FAIL — `Pid` is currently four words.

- [ ] **Step 3: Change the representation**

In `crates/morrow-runtime/src/managed.rs`:

```rust
#[repr(C)]
struct Pid {
    slot: usize,
    generation: u64,
    mailbox: *const Type,
}
```

Add `registry: registry::Registry` to `Session`, replacing `identities`, `next_id` and `used_slots` where they mean identity. Keep `live` as accounting.

Replace `new_pid`, `valid_pid` and `live_pid`:

```rust
unsafe fn new_pid(a: *mut Actor) -> *mut Pid {
    unsafe {
        let pid = allocate::<Pid>();
        *pid = Pid {
            slot: (*a).slot,
            generation: (*a).generation,
            mailbox: (*a).mailbox,
        };
        // The control edge still roots this actor's control storage, because the
        // PID and the actor are in the same domain. Fragments copy PIDs without
        // one; see Task 5.
        memory::control_edge(pid.cast(), a.cast());
        pid
    }
}

/// Resolve identity without dereferencing actor storage.
unsafe fn valid_pid(s: *mut Session, pid: *const Pid) -> bool {
    unsafe {
        !pid.is_null()
            && (*s)
                .registry
                .resolve((*pid).slot, (*pid).generation)
                .is_some()
    }
}

unsafe fn live_pid(s: *mut Session, pid: *const Pid) -> bool {
    unsafe {
        valid_pid(s, pid) && !(*s).stopped && {
            let a = actor_of(s, pid);
            !a.is_null() && (*a).alive && (*a).mailbox == (*pid).mailbox
        }
    }
}

/// Turn identity into storage. Sound only on the scheduler owning that actor;
/// Task 5 removes the last cross-scheduler caller.
unsafe fn actor_of(s: *mut Session, pid: *const Pid) -> *mut Actor {
    unsafe {
        match (*s).registry.resolve((*pid).slot, (*pid).generation) {
            Some(_) => *(*s).identities.add((*pid).slot),
            None => null_mut(),
        }
    }
}
```

Rename `Actor.id` to `Actor.generation` and set it from the `Handle` in `publish_actor`. Update `morrow_managed_supervised_current` to compare `generation` rather than `id`, preserving its documented rule that old PIDs are never redirected.

- [ ] **Step 4: Run the focused tests**

Run: `cargo test -p morrow-runtime pid_carries_identity_only`
Expected: PASS.

- [ ] **Step 5: Run the crate suite and the seeded oracle**

Run: `cargo test -p morrow-runtime`
Expected: PASS, including the existing trace-hash assertion. The hash must be unchanged — this task alters representation, not scheduling order. **If the trace hash moves, stop and find out why before continuing**; a changed hash here means behavior changed, and the whole plan depends on that oracle staying meaningful.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/managed.rs \
        crates/morrow-runtime/src/managed/lifecycle.rs \
        crates/morrow-runtime/src/managed/supervision.rs \
        crates/morrow-runtime/src/managed/tests.rs
git commit -m "refactor(runtime): resolve PIDs through the registry instead of actor pointers"
```

---

### Task 3: `Inbox` — the only thing a sender touches

A sender must not reach actor storage owned by another scheduler. `Inbox` is the shared, reference-counted object that outlives its actor and carries everything `morrow_managed_send` needs: mailbox descriptor, message count, liveness, and the outer queue senders push to.

The two-queue split is BEAM's: senders push onto the outer queue under a short lock, the owning scheduler swaps the whole outer queue out in one operation and works from a private inner queue with no synchronization at all. Per-sender ordering is preserved because one sender's pushes are sequential and the swap reverses uniformly.

**Files:**
- Modify: `crates/morrow-runtime/src/managed/registry.rs`
- Modify: `crates/morrow-runtime/src/managed/registry_tests.rs`
- Modify: `crates/morrow-runtime/src/managed.rs` (`Actor` gains `inbox: Arc<Inbox>`)

**Interfaces:**
- Consumes: `Registry`, `Handle` from Task 1.
- Produces: `Inbox::new(mailbox: Descriptor) -> Arc<Inbox>`, `Inbox::push(&self, message: Delivery) -> Result<(), Delivery>`, `Inbox::take_all(&self) -> Vec<Delivery>`, `Inbox::close(&self)`, `Inbox::is_open(&self) -> bool`, `Inbox::len(&self) -> usize`, `struct Descriptor(*const Type)` with documented `Send`/`Sync`, `Registry::inbox(&self, slot, generation) -> Option<Arc<Inbox>>`.
- `Delivery` is defined in Task 4; until then the queue is generic over a `T: Send` so this task is testable alone.

- [ ] **Step 1: Write the failing test**

Add to `crates/morrow-runtime/src/managed/registry_tests.rs`:

```rust
use super::registry::Inbox;

#[test]
fn an_inbox_outlives_its_actor_and_refuses_delivery_once_closed() {
    let inbox = Inbox::<u64>::new_for_test();
    assert!(inbox.push(7).is_ok());
    inbox.close();
    assert_eq!(
        inbox.push(8),
        Err(8),
        "a closed inbox returns the payload so the sender can release its cost"
    );
    assert!(!inbox.is_open());
    // The queued message is still readable: draining a dead actor's mailbox is
    // the owning scheduler's job, not the sender's.
    assert_eq!(inbox.take_all(), vec![7]);
}

#[test]
fn take_all_preserves_per_sender_order_across_interleaved_senders() {
    let inbox = Inbox::<(u8, u8)>::new_for_test();
    // Two senders interleaved: a1 b1 a2 b2 a3.
    for m in [(0, 1), (1, 1), (0, 2), (1, 2), (0, 3)] {
        inbox.push(m).expect("open");
    }
    let drained = inbox.take_all();
    let a: Vec<u8> = drained.iter().filter(|m| m.0 == 0).map(|m| m.1).collect();
    let b: Vec<u8> = drained.iter().filter(|m| m.0 == 1).map(|m| m.1).collect();
    assert_eq!(a, vec![1, 2, 3], "sender 0 keeps its order");
    assert_eq!(b, vec![1, 2], "sender 1 keeps its order");
}

#[test]
fn take_all_on_an_empty_inbox_allocates_nothing_and_returns_empty() {
    let inbox = Inbox::<u64>::new_for_test();
    assert!(inbox.take_all().is_empty());
    assert_eq!(inbox.len(), 0);
}

#[test]
fn concurrent_senders_lose_no_messages() {
    use std::sync::Arc;
    let inbox = Arc::new(Inbox::<(usize, usize)>::new_for_test());
    let senders: Vec<_> = (0..4)
        .map(|id| {
            let inbox = Arc::clone(&inbox);
            std::thread::spawn(move || {
                for n in 0..256 {
                    inbox.push((id, n)).expect("open");
                }
            })
        })
        .collect();
    for sender in senders {
        sender.join().expect("sender thread");
    }
    let drained = inbox.take_all();
    assert_eq!(drained.len(), 4 * 256, "no message is lost");
    for id in 0..4 {
        let seen: Vec<usize> = drained.iter().filter(|m| m.0 == id).map(|m| m.1).collect();
        assert_eq!(seen, (0..256).collect::<Vec<_>>(), "sender {id} keeps its order");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime inbox`
Expected: FAIL to compile — `Inbox` does not exist.

- [ ] **Step 3: Implement `Inbox`**

Append to `crates/morrow-runtime/src/managed/registry.rs`:

```rust
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// An immutable compiler-owned type descriptor. The compiler emits these into
/// read-only static storage and never mutates them, so sharing one across
/// schedulers is sound; the raw pointer alone is not `Send`, hence this wrapper.
#[derive(Clone, Copy)]
pub(super) struct Descriptor(pub *const super::Type);
// SAFETY: descriptors are immutable for the whole invocation and are validated
// by `cost::descriptor` before any actor can reference them.
unsafe impl Send for Descriptor {}
unsafe impl Sync for Descriptor {}

/// The shared half of an actor: everything a sender on another scheduler may
/// touch. Outlives its actor so a send racing a death resolves by generation
/// rather than by reading freed storage.
pub(super) struct Inbox<T> {
    mailbox: Descriptor,
    open: AtomicBool,
    queued: AtomicUsize,
    // Senders hold this only long enough to push. The owner swaps the whole
    // vector out and then works without synchronization.
    outer: Mutex<Vec<T>>,
}

impl<T> Inbox<T> {
    pub(super) fn new(mailbox: Descriptor) -> Arc<Self> {
        Arc::new(Self {
            mailbox,
            open: AtomicBool::new(true),
            queued: AtomicUsize::new(0),
            outer: Mutex::new(Vec::new()),
        })
    }

    pub(super) fn mailbox(&self) -> *const super::Type {
        self.mailbox.0
    }

    pub(super) fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }

    pub(super) fn len(&self) -> usize {
        self.queued.load(Ordering::Acquire)
    }

    /// Hand back the payload on refusal so the sender can release its charge.
    pub(super) fn push(&self, message: T) -> Result<(), T> {
        let mut outer = self.outer.lock().unwrap_or_else(|e| e.into_inner());
        if !self.open.load(Ordering::Acquire) {
            return Err(message);
        }
        outer.push(message);
        self.queued.store(outer.len(), Ordering::Release);
        Ok(())
    }

    /// Swap the outer queue out in one operation. Order within one sender is
    /// preserved because that sender's pushes are sequential.
    pub(super) fn take_all(&self) -> Vec<T> {
        let mut outer = self.outer.lock().unwrap_or_else(|e| e.into_inner());
        let drained = std::mem::take(&mut *outer);
        self.queued.store(0, Ordering::Release);
        drained
    }

    pub(super) fn close(&self) {
        self.open.store(false, Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn new_for_test() -> Arc<Self> {
        Self::new(Descriptor(std::ptr::null()))
    }
}
```

Give `Entry` an `inbox: Arc<Inbox<Delivery>>` field in Task 4, once `Delivery` exists. For this task add `Registry::attach_inbox(&mut self, slot: usize, inbox: Arc<Inbox<T>>)` only if the compiler requires it; otherwise leave `Registry` untouched and wire the two together in Task 4.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p morrow-runtime inbox`
Expected: PASS, four tests, including the four-thread concurrency case.

- [ ] **Step 5: Run the crate suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/managed/registry.rs \
        crates/morrow-runtime/src/managed/registry_tests.rs
git commit -m "feat(runtime): add the shared actor inbox with a two-queue mailbox"
```

---

### Task 4: Detached heap fragments

The spec's central finding is that `Heap` is a `BTreeMap<usize, Block>` over individually allocated blocks (`memory.rs:62`), so moving an object graph between heaps is a map merge over stable addresses — no relocation, no second copy. This task adds that mechanism to the memory layer, still with no caller.

A fragment is allocate-only from creation until adoption. Nothing roots it and nothing collects it, which is exactly why it is safe to hold one in flight: there is no collector that could free its contents while it has no owner.

**Files:**
- Modify: `crates/morrow-runtime/src/memory.rs` (`Heap`, re-exports near `:14`)
- Modify: `crates/morrow-runtime/src/memory/heaps.rs` (`Slot`, `Domain`, free functions near `:441`)
- Test: `crates/morrow-runtime/src/memory/tests.rs`

**Interfaces:**
- Produces: `memory::Fragment` (opaque, `Send`), `memory::open_fragment() -> usize`, `memory::close_fragment(id: usize) -> Fragment`, `memory::adopt(into: usize, fragment: Fragment)`, `Domain::open_fragment/close_fragment/adopt` behind them.

- [ ] **Step 1: Write the failing test**

Add to `crates/morrow-runtime/src/memory/tests.rs`:

```rust
#[test]
fn an_adopted_fragment_contributes_its_blocks_and_bytes_to_the_receiver() {
    let receiver = unsafe { memory::create_actor_heap(std::ptr::null(), 0) };
    let before = {
        let _scope = memory::enter_heap(receiver);
        memory::stats()
    };

    let fragment_id = memory::open_fragment();
    let written = {
        let _scope = memory::enter_heap(fragment_id);
        let block = memory::alloc(64, false);
        // SAFETY: freshly allocated, zeroed, 64 bytes, exclusively owned here.
        unsafe { block.cast::<u64>().write(0xfeed_beef) };
        block
    };
    let fragment = memory::close_fragment(fragment_id);
    memory::adopt(receiver, fragment);

    let _scope = memory::enter_heap(receiver);
    assert!(
        memory::heap_owns(receiver, written.cast()),
        "the adopted block belongs to the receiver's heap"
    );
    // SAFETY: the block survived adoption; addresses are stable across the merge.
    assert_eq!(unsafe { written.cast::<u64>().read() }, 0xfeed_beef);
    assert!(
        memory::stats().bytes > before.bytes,
        "the fragment's bytes join the receiver's accounting"
    );
    memory::retire_heap(receiver);
}

#[test]
fn a_fragment_is_never_collected_while_in_flight() {
    let fragment_id = memory::open_fragment();
    let survivor = {
        let _scope = memory::enter_heap(fragment_id);
        let block = memory::alloc(32, false);
        // Allocate far past the ordinary 1 MiB threshold. An actor heap would
        // collect here; a fragment must not, because nothing roots it.
        for _ in 0..64 {
            memory::alloc(64 * 1024, true);
        }
        block
    };
    let fragment = memory::close_fragment(fragment_id);
    let receiver = unsafe { memory::create_actor_heap(std::ptr::null(), 0) };
    memory::adopt(receiver, fragment);
    assert!(
        memory::heap_owns(receiver, survivor.cast()),
        "an unrooted fragment allocation must survive until adoption"
    );
    memory::retire_heap(receiver);
}

#[test]
fn adoption_leaves_the_cross_heap_edge_oracle_satisfied() {
    let receiver = unsafe { memory::create_actor_heap(std::ptr::null(), 0) };
    let fragment_id = memory::open_fragment();
    {
        let _scope = memory::enter_heap(fragment_id);
        memory::alloc(48, false);
    }
    memory::adopt(receiver, memory::close_fragment(fragment_id));
    assert_eq!(
        memory::verify_heap_edges(),
        Ok(()),
        "adopted payload carries no edge out of its new heap"
    );
    memory::retire_heap(receiver);
}

#[test]
fn fragments_are_send_so_they_can_cross_schedulers() {
    fn assert_send<T: Send>() {}
    assert_send::<memory::Fragment>();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime fragment`
Expected: FAIL to compile — `open_fragment` does not exist.

- [ ] **Step 3: Implement the merge**

In `crates/morrow-runtime/src/memory.rs`, add to `impl Heap`:

```rust
/// Take ownership of another heap's blocks.
///
/// Addresses are stable allocations, so adoption is a map merge: no pointer
/// rewriting and no second copy. `other` is left empty, so its `Drop` destroys
/// nothing.
fn merge(&mut self, other: &mut Heap) {
    self.blocks.append(&mut other.blocks);
    self.bytes = self.bytes.saturating_add(other.bytes);
    other.bytes = 0;
}
```

Add the public type:

```rust
/// A detached heap carrying one in-flight payload between schedulers.
///
/// Allocate-only from creation until adoption: nothing roots a fragment and
/// nothing collects it, so its contents cannot be freed while it has no owner.
/// Exactly one owner holds a fragment at a time — the sender until it is pushed,
/// the receiving scheduler afterwards.
pub struct Fragment {
    heap: Heap,
}
```

In `crates/morrow-runtime/src/memory/heaps.rs`, add `fragment: bool` to `Slot` and:

```rust
impl Domain {
    /// Open an allocate-only heap with no collection threshold.
    pub(crate) fn open_fragment(&mut self) -> usize {
        self.next_heap += 1;
        let id = self.next_heap;
        let mut slot = Slot::invocation();
        slot.fragment = true;
        // Nothing roots a fragment, so it must never collect.
        slot.heap.threshold = usize::MAX;
        self.slots.insert(id, slot);
        id
    }

    pub(crate) fn close_fragment(&mut self, id: usize) -> Fragment {
        let slot = self
            .slots
            .remove(&id)
            .expect("close_fragment on a live fragment id");
        assert!(slot.fragment, "close_fragment on an actor heap");
        assert_ne!(self.active, id, "close_fragment while the fragment is active");
        Fragment { heap: slot.heap }
    }

    pub(crate) fn adopt(&mut self, into: usize, mut fragment: Fragment) {
        let slot = self.slots.get_mut(&into).expect("adopt into a live heap");
        slot.heap.merge(&mut fragment.heap);
    }
}
```

Skip fragments in `verify_edges` (they hold no control edges and are not yet in any actor's heap) by adding `if slot.fragment { continue; }` beside the existing `if id == 0 { continue; }`.

Export from `memory.rs` beside the existing `heaps::{...}` re-export at `:14`, and add thread-local free functions in `heaps.rs` beside `enter`/`retire` that delegate to the current `Domain`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p morrow-runtime fragment`
Expected: PASS, four tests.

- [ ] **Step 5: Run the crate suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS, including `verify_edges` and its negative case.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/memory.rs \
        crates/morrow-runtime/src/memory/heaps.rs \
        crates/morrow-runtime/src/memory/tests.rs
git commit -m "feat(runtime): add detached heap fragments adopted by a map merge"
```

---

### Task 5: Send copies into a fragment instead of the receiver's heap

This is the ownership crux. After this task `morrow_managed_send` no longer executes `memory::enter_heap((*a).heap)` (`lifecycle.rs:195`) and no longer touches receiver storage at all. Still single-threaded — but the send path is now shaped so that a second thread changes nothing about it.

The inner queue stays exactly as it is: at adoption the owning scheduler merges the fragment and then allocates a `Message` in the actor's own heap and links it, so `managed/receive.rs`, selectors and timeouts are untouched.

**Files:**
- Modify: `crates/morrow-runtime/src/managed/lifecycle.rs:162-218` (`morrow_managed_send`)
- Modify: `crates/morrow-runtime/src/managed/scheduler.rs` (adoption before `step`)
- Modify: `crates/morrow-runtime/src/managed/registry.rs` (`Delivery`, `Entry.inbox`)
- Modify: `crates/morrow-runtime/src/managed/copy.rs` (PIDs copied into a fragment carry no control edge)
- Test: `crates/morrow-runtime/src/managed/tests.rs`

**Interfaces:**
- Consumes: `memory::Fragment`, `memory::open_fragment/close_fragment/adopt` (Task 4); `Inbox`, `Registry` (Tasks 1 and 3).
- Produces: `struct Delivery { fragment: memory::Fragment, value: i64, cost: usize, enqueued: u64 }`; `Registry::inbox(&self, slot: usize, generation: u64) -> Option<Arc<Inbox<Delivery>>>`, which is where `Entry` finally gains its `inbox` field; `unsafe fn adopt_inbox(s: *mut Session, a: *mut Actor)` called by the scheduler before stepping an actor.

- [ ] **Step 1: Write the failing test**

Add to `crates/morrow-runtime/src/managed/tests.rs`:

```rust
#[test]
fn send_never_enters_the_receiver_heap() {
    // The oracle is structural: the sender's active heap must be unchanged across
    // the whole send, which is what makes the path safe to run on another thread.
    let observed = std::cell::Cell::new(Vec::<usize>::new());
    memory::observe_active_heap_for_test(|id| {
        let mut seen = observed.take();
        seen.push(id);
        observed.set(seen);
    });
    let report = simulation::run(simulation::Config {
        seed: 0x5eed_0005,
        steps: 2048,
    })
    .expect("scenario completes");
    assert!(report.delivered > 0, "the scenario must actually deliver messages");
    memory::clear_active_heap_observer_for_test();
    // Every heap made active during a send is either the sender's or a fragment.
    assert!(
        memory::observed_only_sender_and_fragment_heaps_for_test(),
        "send entered a receiver heap"
    );
}

#[test]
fn a_sent_graph_keeps_its_internal_sharing_after_adoption() {
    // The existing guarantee, restated against the fragment path: this is the
    // assertion `send_copies_nested_graph_and_preserves_internal_sharing` makes,
    // and it must survive the change of destination heap.
    let report = simulation::run(simulation::Config {
        seed: 0x5eed_0006,
        steps: 4096,
    })
    .expect("scenario completes");
    assert_eq!(report.final_heap_objects, 0, "no adopted block leaks");
    assert_eq!(report.final_messages, 0);
}

#[test]
fn a_message_to_a_dead_generation_is_dropped_and_its_cost_released() {
    let report = simulation::run(simulation::Config {
        seed: 0x5eed_0007,
        steps: 4096,
    })
    .expect("scenario completes");
    assert!(report.churn > 0, "actors must die during this scenario");
    assert_eq!(
        report.final_heap_bytes, 0,
        "a refused delivery releases its charge and its fragment"
    );
}
```

If a structural observer hook is more machinery than it earns, replace the first test with a direct unit test in `lifecycle.rs`'s test module that calls `morrow_managed_send` with a receiver whose heap id is recorded, and asserts `memory::heap_owns(receiver_heap, copied_value)` is **false** immediately after send and **true** after `adopt_inbox`. That is the same property with less apparatus — prefer it if the hook does not fall out naturally.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime send_never_enters_the_receiver_heap`
Expected: FAIL — send still enters the receiver heap.

- [ ] **Step 3: Rewrite the send path**

In `crates/morrow-runtime/src/managed/registry.rs`:

```rust
/// One in-flight message: its payload heap plus the accounting the receiver
/// needs to link it. The fragment is owned by exactly one side at a time.
pub(super) struct Delivery {
    pub fragment: crate::memory::Fragment,
    pub value: i64,
    pub cost: usize,
    pub enqueued: u64,
}
```

Replace the body of `morrow_managed_send` between the cost preflight and the mailbox link (`lifecycle.rs:195-213`) with:

```rust
let Some(inbox) = (*s).registry.inbox((*pid).slot, (*pid).generation) else {
    release(s, cost);
    return abi::result_err(3);
};
if !inbox.is_open() || inbox.len() >= MAILBOX {
    release(s, cost);
    return abi::result_err(4);
}
let fragment_id = memory::open_fragment();
let copied = {
    let _fragment_scope = memory::enter_heap(fragment_id);
    copy::value(s, ty, value)
};
let delivery = registry::Delivery {
    fragment: memory::close_fragment(fragment_id),
    value: copied.value,
    cost,
    enqueued: now,
};
if inbox.push(delivery).is_err() {
    // The actor died between the registry lookup and the push. Its generation is
    // dead, so the message is dropped exactly as a send to a dead PID is today.
    release(s, cost);
    return abi::result_err(3);
}
```

Task 6 adds one in-flight counter increment at the end of this function and the matching decrement in `adopt_inbox` below. Leave both out here; Task 6 shows exactly where they go.

Note that the receiver's `(*a).messages`, `(*s).messages` and `enqueue(a)` are **not** touched here. They move to adoption, which runs on the owning scheduler:

```rust
/// Merge every queued fragment into the actor's own heap, then link the messages.
/// Runs only on the scheduler owning this actor.
pub(super) unsafe fn adopt_inbox(s: *mut Session, a: *mut Actor) {
    unsafe {
        let inbox = (*a).inbox.clone();
        for delivery in inbox.take_all() {
            memory::adopt((*a).heap, delivery.fragment);
            let message = {
                let _scope = memory::enter_heap((*a).heap);
                allocate::<Message>()
            };
            *message = Message {
                next: null_mut(),
                value: delivery.value,
                cost: delivery.cost,
                enqueued: delivery.enqueued,
            };
            if (*a).last.is_null() {
                (*a).first = message;
            } else {
                (*(*a).last).next = message;
            }
            (*a).last = message;
            (*a).messages += 1;
            (*s).messages += 1;
            if (*a).waiting && ((*a).deadline == u64::MAX || delivery.enqueued < (*a).deadline) {
                enqueue(a);
            }
        }
    }
}
```

Call `adopt_inbox` from `scheduler::step` before dispatching, and from `morrow_managed_run`'s loop head for actors woken by a remote push.

In `copy.rs`, a copied `Pid` writes `slot`, `generation` and `mailbox` and registers **no** `control_edge`: the fragment has no actor storage to root, and after adoption the PID names an identity rather than an address. Add a comment saying exactly that — it is the one place where the edge classes differ from `new_pid`.

In `scheduler::finish`, replace the mailbox drain with `(*a).inbox.close()` followed by draining both the inner list and `take_all()`, releasing each cost. A closed inbox refuses later senders, which is what makes the dead-generation drop cheap.

- [ ] **Step 4: Run the focused tests**

Run: `cargo test -p morrow-runtime send`
Expected: PASS, including the pre-existing `send_copies_nested_graph_and_preserves_internal_sharing`.

- [ ] **Step 5: Run the crate suite and check the trace hash**

Run: `cargo test -p morrow-runtime`
Expected: PASS. **The trace hash may legitimately change in this task** — adoption moves message linking from send time to step time, which reorders observable delivery. If it changes, record the new hash and say so in the commit message; if it does *not* change, say that too. Do not update the expected hash without stating which of the two happened.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/managed/lifecycle.rs \
        crates/morrow-runtime/src/managed/scheduler.rs \
        crates/morrow-runtime/src/managed/registry.rs \
        crates/morrow-runtime/src/managed/copy.rs \
        crates/morrow-runtime/src/managed/tests.rs
git commit -m "feat(runtime): carry sent payloads in detached fragments the receiver adopts"
```

---

### Task 6: Global termination detection

`idle` (`scheduler.rs:142`) infers deadlock from local emptiness and raises fault 10. That inference is about to become false: a scheduler with an empty queue may be waiting on a message another thread is mid-send. This task replaces the inference with an explicit accounting of outstanding work, while there is still only one scheduler and therefore a single-threaded oracle to check it against.

A message is **in flight** from the moment its cost is charged until the moment it is adopted. Termination requires: no runnable actors on any scheduler, no pending timer, and no in-flight messages. Only the last scheduler to park evaluates it.

**Files:**
- Create: `crates/morrow-runtime/src/managed/quiesce.rs`
- Create: `crates/morrow-runtime/src/managed/quiesce_tests.rs`
- Modify: `crates/morrow-runtime/src/managed/scheduler.rs:142-175` (`idle`)
- Modify: `crates/morrow-runtime/src/managed/lifecycle.rs` (send), `scheduler.rs` (`adopt_inbox`, `enqueue`, `dequeue`)

**Interfaces:**
- Produces: `struct Quiesce` with `runnable: AtomicUsize`, `in_flight: AtomicUsize`, `parked: AtomicUsize`; `Quiesce::message_sent(&self)`, `message_adopted(&self)`, `actor_runnable(&self)`, `actor_dequeued(&self)`, `park(&self, schedulers: usize) -> Parked`, `enum Parked { Work, Terminated, Waiting }`.

- [ ] **Step 1: Write the failing test**

Create `crates/morrow-runtime/src/managed/quiesce_tests.rs`:

```rust
use super::quiesce::{Parked, Quiesce};

#[test]
fn an_in_flight_message_prevents_termination_even_with_no_runnable_actors() {
    let q = Quiesce::default();
    q.message_sent();
    assert_eq!(
        q.park(1),
        Parked::Waiting,
        "a charged but unadopted message is outstanding work"
    );
    q.message_adopted();
    q.actor_runnable();
    assert_eq!(q.park(1), Parked::Work);
}

#[test]
fn termination_is_reported_only_when_nothing_is_outstanding() {
    let q = Quiesce::default();
    assert_eq!(q.park(1), Parked::Terminated);
}

#[test]
fn the_last_scheduler_to_park_is_the_one_that_sees_termination() {
    let q = Quiesce::default();
    assert_eq!(q.park(2), Parked::Waiting, "one of two schedulers still runs");
    assert_eq!(q.park(2), Parked::Terminated, "the second park completes the set");
}

#[test]
fn a_wakeup_after_parking_retracts_termination() {
    let q = Quiesce::default();
    assert_eq!(q.park(2), Parked::Waiting);
    q.actor_runnable();
    assert_eq!(
        q.park(2),
        Parked::Work,
        "work appearing while a scheduler is parked must unpark rather than terminate"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime quiesce`
Expected: FAIL to compile — `quiesce` does not exist.

- [ ] **Step 3: Implement the counters**

Create `crates/morrow-runtime/src/managed/quiesce.rs`:

```rust
//! Global termination detection.
//!
//! Local emptiness does not imply global termination once more than one
//! scheduler exists: an idle scheduler may be waiting on a message another
//! thread has charged but not yet delivered. Outstanding work is therefore
//! counted explicitly, and only the last scheduler to park evaluates the sum.
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Default)]
pub(super) struct Quiesce {
    runnable: AtomicUsize,
    in_flight: AtomicUsize,
    parked: AtomicUsize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Parked {
    /// Work appeared; resume immediately.
    Work,
    /// Nothing is outstanding anywhere: the invocation is done.
    Terminated,
    /// This scheduler has nothing to do, but work exists elsewhere.
    Waiting,
}

impl Quiesce {
    /// A message is outstanding from charge until adoption.
    pub(super) fn message_sent(&self) {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
    }

    pub(super) fn message_adopted(&self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }

    pub(super) fn actor_runnable(&self) {
        self.runnable.fetch_add(1, Ordering::AcqRel);
        // Retract any in-progress termination decision.
        self.parked.store(0, Ordering::Release);
    }

    pub(super) fn actor_dequeued(&self) {
        self.runnable.fetch_sub(1, Ordering::AcqRel);
    }

    pub(super) fn park(&self, schedulers: usize) -> Parked {
        if self.runnable.load(Ordering::Acquire) != 0 {
            self.parked.store(0, Ordering::Release);
            return Parked::Work;
        }
        let parked = self.parked.fetch_add(1, Ordering::AcqRel) + 1;
        if parked < schedulers {
            return Parked::Waiting;
        }
        // Re-check after becoming the last parked scheduler: a sender may have
        // charged a message between the first load and here.
        if self.in_flight.load(Ordering::Acquire) != 0
            || self.runnable.load(Ordering::Acquire) != 0
        {
            self.parked.store(0, Ordering::Release);
            return Parked::Waiting;
        }
        Parked::Terminated
    }
}
```

Wire it: `message_sent` at the end of `morrow_managed_send` after a successful `push`; `message_adopted` in `adopt_inbox` after each link; `actor_runnable` in `enqueue`; `actor_dequeued` in `dequeue`. In `idle`, replace the "empty queue and no deadline means fault 10" branch with `Parked::Terminated => return false` and `Parked::Waiting =>` sleep-or-wait, preserving fault 10 for the genuine no-work-no-timer case at one scheduler so existing deadlock tests keep their meaning.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p morrow-runtime quiesce`
Expected: PASS, four tests.

- [ ] **Step 5: Run the crate suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS, including whichever existing test asserts fault 10 on deadlock. That test is the oracle proving this change did not silently convert deadlock into termination — **do not relax it**.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/managed/quiesce.rs \
        crates/morrow-runtime/src/managed/quiesce_tests.rs \
        crates/morrow-runtime/src/managed/scheduler.rs \
        crates/morrow-runtime/src/managed/lifecycle.rs \
        crates/morrow-runtime/src/managed.rs
git commit -m "feat(runtime): count outstanding work instead of inferring deadlock locally"
```

---

### Task 7: Split `Session` into per-scheduler and shared state

`Session` currently fuses a run queue, an identity registry and invocation accounting (`managed.rs:84`). Tasks 1-6 removed identity and outstanding-work tracking from it. This task separates what remains: the run queue and deadlines become per-scheduler; `live`, `messages`, `retained` and `stopped` become shared atomics.

The limits must stay exact. A program that fails on the 64 MiB ceiling today must fail identically at eight schedulers, so `charge` becomes a compare-exchange loop rather than a per-scheduler budget.

**Files:**
- Modify: `crates/morrow-runtime/src/managed.rs:84-100` (`Session`), `:185-199` (`charge`, `release`)
- Modify: `crates/morrow-runtime/src/managed/scheduler.rs` (queue operations take a `*mut Scheduler`)
- Test: `crates/morrow-runtime/src/managed/tests.rs`

**Interfaces:**
- Produces: `struct Scheduler { index: usize, domain_cursor: usize, first: *mut Actor, last: *mut Actor, next_deadline: u64, remote: Mutex<Vec<*mut Actor>> }`; `struct Session { schedulers: Vec<Scheduler>, registry: Registry, quiesce: Quiesce, next_placement: AtomicUsize, live: AtomicUsize, messages: AtomicUsize, retained: AtomicUsize, stopped: AtomicBool, root: Exec, functions: ..., function_count: ... }`. `next_placement` is unused until Task 8 reads it.
- `Exec` keeps its `#[repr(C)]` layout and its `session`/`actor`/`fault` fields unchanged — it is compiler-visible.

- [ ] **Step 1: Write the failing test**

Add to `crates/morrow-runtime/src/managed/tests.rs`:

```rust
#[test]
fn the_byte_ceiling_stays_exact_under_concurrent_charges() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    // charge/release must behave as one global budget, not N per-scheduler ones.
    // Over-admitting by even one allocation changes observable failure.
    let retained = Arc::new(AtomicUsize::new(0));
    let admitted = Arc::new(AtomicUsize::new(0));
    let ceiling = 1024;
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let retained = Arc::clone(&retained);
            let admitted = Arc::clone(&admitted);
            std::thread::spawn(move || {
                for _ in 0..1024 {
                    if super::charge_atomic(&retained, ceiling, 16) {
                        admitted.fetch_add(16, Ordering::AcqRel);
                    }
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("worker");
    }
    assert_eq!(
        admitted.load(Ordering::Acquire),
        retained.load(Ordering::Acquire),
        "every admitted charge is accounted exactly once"
    );
    assert!(
        retained.load(Ordering::Acquire) <= ceiling,
        "the ceiling is never exceeded, even transiently"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime byte_ceiling`
Expected: FAIL to compile — `charge_atomic` does not exist.

- [ ] **Step 3: Make accounting atomic and split the queue**

In `crates/morrow-runtime/src/managed.rs`:

```rust
/// One global budget shared by every scheduler. A compare-exchange loop keeps the
/// ceiling exact: `fetch_add` followed by a check would admit transiently and
/// change observable failure.
fn charge_atomic(retained: &AtomicUsize, ceiling: usize, cost: usize) -> bool {
    let mut current = retained.load(Ordering::Acquire);
    loop {
        if cost > ceiling - current {
            return false;
        }
        match retained.compare_exchange_weak(
            current,
            current + cost,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}
```

`charge(s, cost)` becomes `charge_atomic(&(*s).retained, BYTES, cost)`; `release` becomes `fetch_sub` with the existing debit assertion preserved as a `debug_assert` plus a saturating guard in release builds.

Move `first`, `last` and `next_deadline` from `Session` onto `Scheduler`, and give `Scheduler` a `remote: Mutex<Vec<*mut Actor>>` for cross-thread wakeups, spliced at the top of the run loop. `enqueue`/`dequeue`/`wake_due`/`refresh` take `*mut Scheduler` instead of `*mut Session`. With one scheduler this is a pure rename; the ordering must not change.

- [ ] **Step 4: Run the focused test**

Run: `cargo test -p morrow-runtime byte_ceiling`
Expected: PASS.

- [ ] **Step 5: Run the crate suite and check the trace hash**

Run: `cargo test -p morrow-runtime`
Expected: PASS with the trace hash **unchanged** from Task 5's recorded value. At one scheduler this task is a rename; a moved hash means the queue split changed ordering, which is a defect, not a new baseline.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/managed.rs \
        crates/morrow-runtime/src/managed/scheduler.rs \
        crates/morrow-runtime/src/managed/tests.rs
git commit -m "refactor(runtime): separate per-scheduler queues from shared invocation accounting"
```

---

### Task 8: Placement at spawn and remote materialization

Step 1's design says actors are "pinned to their spawning scheduler". Taken literally that yields no parallelism: programs spawn from main on scheduler 0, so every actor lands there. Placement and pinning are therefore separate — actors are **placed** round-robin and **pinned** for life.

Supervised children are the exception: `(*a).supervisor` points at a `Supervisor` owned by the supervising domain, so a child placed elsewhere would be exactly the cross-domain pointer `verify_edges` forbids. Co-location is enforced, not incidental.

**Files:**
- Create: `crates/morrow-runtime/src/managed/placement.rs`
- Modify: `crates/morrow-runtime/src/managed/lifecycle.rs:100-160` (`morrow_managed_spawn`)
- Modify: `crates/morrow-runtime/src/managed/supervision.rs` (restart places on the supervisor's scheduler)
- Test: `crates/morrow-runtime/src/managed/tests.rs`

**Interfaces:**
- Consumes: `Registry::issue(scheduler)` (Task 1), `Fragment` (Task 4), `Scheduler.remote` (Task 7).
- Produces: `placement::next(s: *mut Session) -> usize`, `placement::colocated(supervisor: *mut Supervisor) -> usize`, `enum Request { Spawn { handle: Handle, frame: Fragment, value: i64, mailbox: Descriptor } }` pushed onto the target scheduler's remote queue.

- [ ] **Step 1: Write the failing test**

Add to `crates/morrow-runtime/src/managed/tests.rs`:

```rust
#[test]
fn unsupervised_spawns_distribute_round_robin_across_schedulers() {
    let report = simulation::run(simulation::Config {
        seed: 0x5eed_0008,
        steps: 4096,
        schedulers: 4,
    })
    .expect("scenario completes");
    let placed = report.placement.expect("placement is reported");
    assert_eq!(placed.len(), 4, "every scheduler is reported, including empty ones");
    assert!(
        placed.iter().all(|&count| count > 0),
        "round-robin placement must reach every scheduler, got {placed:?}"
    );
}

#[test]
fn supervised_children_are_placed_on_their_supervisors_scheduler() {
    let report = simulation::run(simulation::Config {
        seed: 0x5eed_0009,
        steps: 4096,
        schedulers: 4,
    })
    .expect("scenario completes");
    assert!(report.restarts > 0, "the scenario must restart supervised children");
    assert_eq!(
        report.cross_scheduler_supervision, 0,
        "a supervised child on another scheduler would be a cross-domain supervisor pointer"
    );
    assert_eq!(memory::verify_heap_edges(), Ok(()));
}
```

`Config.schedulers` and `Report.placement`/`cross_scheduler_supervision` are added in Task 9. Write these two tests in Task 9's commit if Task 8 lands first; the plan keeps them here because they are placement's oracles, not the driver's.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime placement`
Expected: FAIL — placement is not implemented and everything lands on scheduler 0.

- [ ] **Step 3: Implement placement**

Create `crates/morrow-runtime/src/managed/placement.rs`:

```rust
//! Where a new actor runs, decided once at spawn and never revisited.
//!
//! Round-robin rather than least-loaded: without migration there is no way to
//! correct a bad decision, and reading other schedulers' queue depths would put a
//! shared read on the spawn path for an estimate that is stale on arrival.
use super::*;

pub(super) unsafe fn next(s: *mut Session) -> usize {
    unsafe {
        let count = (*s).schedulers.len();
        (*s).next_placement.fetch_add(1, Ordering::AcqRel) % count
    }
}

/// A supervised child must share its supervisor's domain: the `Supervisor` it
/// points at is allocated there, and a cross-domain pointer is exactly what
/// `Domain::verify_edges` forbids.
pub(super) unsafe fn colocated(s: *mut Session, supervisor: *mut Supervisor) -> usize {
    unsafe {
        let _ = s;
        (*supervisor).scheduler
    }
}
```

Give `Supervisor` a `scheduler: usize` field set at `morrow_managed_supervise` time. In `morrow_managed_spawn`, choose the target with `placement::next`, and when it differs from the current scheduler, copy the initializer frame into a fragment and push a `Request::Spawn` onto the target's `remote` queue instead of allocating the `Actor` locally. The registry issues the handle and inbox synchronously, so the returned PID is usable before the actor materializes — a message sent to it is adopted in order after the spawn request.

- [ ] **Step 4: Run the focused tests**

Run: `cargo test -p morrow-runtime placement`
Expected: PASS.

- [ ] **Step 5: Run the crate suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS, with the one-scheduler trace hash unchanged — at `schedulers: 1` round-robin is the identity.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/managed/placement.rs \
        crates/morrow-runtime/src/managed/lifecycle.rs \
        crates/morrow-runtime/src/managed/supervision.rs \
        crates/morrow-runtime/src/managed.rs \
        crates/morrow-runtime/src/managed/tests.rs
git commit -m "feat(runtime): place actors round-robin at spawn and co-locate supervised children"
```

---

### Task 9: The deterministic multi-scheduler driver

Determinism is a property of the simulation driver, not of the runtime (Decision156). Under the `simulation` feature, N schedulers become N cooperative state machines advanced by a seeded driver on **one** OS thread, with a decision point wherever real threads would interleave.

There are exactly five such points, and every cross-scheduler operation funnels through them by construction: outer-queue splice, remote run-queue splice, registry lookup, park, unpark. That is why the driver covers the interleaving surface without instrumenting the hot path, and why it costs nothing in production builds.

This is the task that makes the rest of the plan debuggable: after it, a cross-scheduler ordering defect is a reproducible hash mismatch rather than an intermittent report.

**Files:**
- Modify: `crates/morrow-runtime/src/managed/simulation/scenario.rs` (`Config`, `Report`, `drive`)
- Modify: `crates/morrow-runtime/src/managed/simulation.rs` (`State` gains the schedule cursor)
- Modify: `crates/morrow-runtime/src/managed/scheduler.rs` (yield at the five decision points)
- Test: `crates/morrow-runtime/src/managed/tests.rs`

**Interfaces:**
- Consumes: `Quiesce::park` (Task 6), `Scheduler.remote` (Task 7), `placement::next` (Task 8).
- Produces: `Config { seed: u64, steps: u32, schedulers: usize }`; `Report` gains `placement: Option<Vec<usize>>`, `cross_scheduler_supervision: usize`, `cross_scheduler_sends: u64`; `simulation::yield_point(kind: Point)` with `enum Point { OuterSplice, RemoteSplice, Resolve, Park, Unpark }`.

- [ ] **Step 1: Write the failing test**

Add to `crates/morrow-runtime/src/managed/tests.rs`:

```rust
#[test]
fn a_seed_replays_identically_at_every_scheduler_count() {
    for schedulers in [1usize, 2, 4, 8] {
        let config = simulation::Config {
            seed: 0x5eed_000a,
            steps: 8192,
            schedulers,
        };
        let first = simulation::run(config).expect("scenario completes");
        let second = simulation::run(config).expect("scenario completes");
        assert_eq!(
            first, second,
            "seed {:#x} at {schedulers} schedulers must replay exactly",
            config.seed
        );
    }
}

#[test]
fn scheduler_count_changes_the_interleaving_rather_than_being_ignored() {
    let one = simulation::run(simulation::Config {
        seed: 0x5eed_000a,
        steps: 8192,
        schedulers: 1,
    })
    .expect("scenario completes");
    let eight = simulation::run(simulation::Config {
        seed: 0x5eed_000a,
        steps: 8192,
        schedulers: 8,
    })
    .expect("scenario completes");
    assert_ne!(
        one.trace_hash, eight.trace_hash,
        "if the hash is identical the driver is not actually interleaving schedulers"
    );
    assert!(
        eight.cross_scheduler_sends > 0,
        "eight schedulers must produce real cross-scheduler traffic"
    );
    // Both must still be correct runs, not merely different ones.
    assert_eq!(one.final_live, 0);
    assert_eq!(eight.final_live, 0);
    assert_eq!(one.delivered, eight.delivered, "no message is lost by interleaving");
}

#[test]
fn the_single_scheduler_trace_hash_is_unchanged_by_the_driver() {
    let report = simulation::run(simulation::Config {
        seed: 0x5eed_0001,
        steps: 8192,
        schedulers: 1,
    })
    .expect("scenario completes");
    // Replace with the hash recorded at Task 5 — see that task's Step 5 note.
    assert_eq!(
        report.trace_hash, 0x01a5_5a00_46de_5614,
        "one scheduler must behave exactly as step 1's runtime did"
    );
}
```

The third test is the load-bearing one. **If it fails, the driver has changed single-scheduler behavior and the defect is in the driver, not the oracle.**

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime replays_identically`
Expected: FAIL to compile — `Config` has no `schedulers` field.

- [ ] **Step 3: Add the schedule cursor and the five decision points**

In `simulation/scenario.rs`, add `schedulers: usize` to `Config` (validated to `1..=64`), and the three new `Report` fields. In `drive`, construct N `Scheduler` values in one `Session` and advance them by a seeded cursor:

```rust
/// Choose which logical scheduler advances next. The seed alone determines the
/// whole interleaving, so a report is reproducible from `Config` and nothing else.
fn next_scheduler(state: &mut ScheduleState, schedulers: usize) -> usize {
    // xorshift64*, matching the generator the scenario already uses for events.
    state.cursor ^= state.cursor << 13;
    state.cursor ^= state.cursor >> 7;
    state.cursor ^= state.cursor << 17;
    (state.cursor % schedulers as u64) as usize
}
```

At each of the five points, production builds compile to nothing:

```rust
#[cfg(any(test, feature = "simulation"))]
#[inline]
pub(super) unsafe fn yield_point(s: *mut Session, point: Point) {
    unsafe {
        if (*s).simulation.enabled {
            simulation::record_and_switch(s, point);
        }
    }
}
#[cfg(not(any(test, feature = "simulation")))]
#[inline(always)]
pub(super) unsafe fn yield_point(_s: *mut Session, _point: Point) {}
```

Fold `point` and the advancing scheduler index into the trace hash so an ordering change is visible in the hash rather than only in the outcome.

- [ ] **Step 4: Run the focused tests**

Run: `cargo test -p morrow-runtime scheduler_count scheduler_trace_hash replays_identically`
Expected: PASS, three tests.

- [ ] **Step 5: Run the crate suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS. Add `schedulers: 1` to every pre-existing `Config` literal — there is no `Default`, deliberately, so that every call site states its scheduler count.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/managed/simulation.rs \
        crates/morrow-runtime/src/managed/simulation/scenario.rs \
        crates/morrow-runtime/src/managed/scheduler.rs \
        crates/morrow-runtime/src/managed/tests.rs
git commit -m "test(runtime): interleave logical schedulers deterministically from a seed"
```

---

### Task 10: Interleaving-invariant oracles

Replay proves a seed reproduces. It does not prove the result is *correct*. These are the properties that must hold under every interleaving, asserted across a seed sweep — the oracles that would catch a lost message or a torn counter that happens to reproduce identically.

**Files:**
- Modify: `crates/morrow-runtime/src/managed/simulation/scenario.rs` (per-pair sequence tracking)
- Test: `crates/morrow-runtime/src/managed/tests.rs`

**Interfaces:**
- Consumes: `Report` fields from Task 9.
- Produces: `Report.ordering_violations: usize`, `Report.lost: u64`, `Report.duplicated: u64`, `Report.fragments_outstanding: usize`, `Report.sent: u64`, `Report.parks_with_inflight: u64`, `Report.spurious_deadlocks: usize`; `simulation::run_with_forced_reorder_for_test`, `simulation::run_with_park_pressure_for_test`.

- [ ] **Step 1: Write the failing test**

Add to `crates/morrow-runtime/src/managed/tests.rs`:

```rust
#[test]
fn every_invariant_holds_across_a_seed_sweep_at_every_scheduler_count() {
    for seed in 0..32u64 {
        for schedulers in [1usize, 2, 4, 8] {
            let report = simulation::run(simulation::Config {
                seed: 0x5eed_0100 + seed,
                steps: 4096,
                schedulers,
            })
            .unwrap_or_else(|e| panic!("seed {seed} at {schedulers} schedulers: {e}"));

            assert_eq!(
                report.ordering_violations, 0,
                "seed {seed}/{schedulers}: messages between one pair must arrive in send order"
            );
            assert_eq!(report.lost, 0, "seed {seed}/{schedulers}: a message was lost");
            assert_eq!(
                report.duplicated, 0,
                "seed {seed}/{schedulers}: a message was delivered twice"
            );
            assert_eq!(
                report.fragments_outstanding, 0,
                "seed {seed}/{schedulers}: a fragment outlived adoption"
            );
            assert_eq!(
                report.final_messages, 0,
                "seed {seed}/{schedulers}: message accounting did not return to zero"
            );
            assert_eq!(
                report.final_heap_bytes, 0,
                "seed {seed}/{schedulers}: retained bytes did not return to zero"
            );
            assert_eq!(report.final_live, 0, "seed {seed}/{schedulers}: an actor leaked");
        }
    }
}

#[test]
fn a_scheduler_parking_while_a_message_is_in_flight_neither_hangs_nor_faults() {
    // Spec oracle 5. Task 6 tests the counters in isolation; this drives the real
    // scheduler into the shape that made them necessary — every scheduler out of
    // local work while a delivery is charged but not yet adopted.
    for seed in 0..16u64 {
        let report = simulation::run_with_park_pressure_for_test(simulation::Config {
            seed: 0x5eed_0300 + seed,
            steps: 4096,
            schedulers: 4,
        })
        .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert!(
            report.parks_with_inflight > 0,
            "seed {seed}: the scenario never reached the shape it is meant to test"
        );
        assert_eq!(
            report.spurious_deadlocks, 0,
            "seed {seed}: a scheduler declared deadlock while a message was in flight"
        );
        assert_eq!(report.final_live, 0, "seed {seed}: the run did not terminate cleanly");
        assert_eq!(report.delivered, report.sent, "seed {seed}: an in-flight message was dropped");
    }
}

#[test]
fn the_ordering_oracle_can_fail() {
    // An oracle that cannot fail proves nothing. Deliver one pair out of order on
    // purpose and assert the counter notices, mirroring the deliberately failing
    // negative case that guards `verify_edges`.
    let report = simulation::run_with_forced_reorder_for_test(simulation::Config {
        seed: 0x5eed_0200,
        steps: 1024,
        schedulers: 2,
    })
    .expect("scenario completes");
    assert!(
        report.ordering_violations > 0,
        "a deliberately reordered pair must be reported"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime invariant`
Expected: FAIL to compile — the `Report` fields do not exist.

- [ ] **Step 3: Track the invariants**

In the scenario driver, stamp each sent message with `(sender_slot, sequence)` and record the highest sequence delivered per `(sender, receiver)` pair. A delivery whose sequence is lower than the recorded high-water mark for its pair increments `ordering_violations`. Count sends and adoptions to derive `lost` and `duplicated`. Track open fragment ids to derive `fragments_outstanding`.

Add `Report.parks_with_inflight`, `Report.spurious_deadlocks` and `Report.sent`, counted in the driver: increment `parks_with_inflight` whenever `Quiesce::park` returns `Waiting` with a nonzero in-flight count, and `spurious_deadlocks` whenever fault 10 is raised while any message is in flight.

Add two `#[cfg(test)]` drivers beside `force_control_edge` (`heaps.rs:80`), which is the same idea applied to the edge oracle:

- `run_with_forced_reorder_for_test`, which swaps the first two entries of one `take_all` result exactly once, so the ordering oracle is demonstrably able to fail.
- `run_with_park_pressure_for_test`, which biases the schedule cursor toward parking a scheduler between a send's charge and its adoption, so the in-flight window is actually hit rather than hoped for.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p morrow-runtime invariant ordering_oracle parking_while_a_message`
Expected: PASS. The sweep is 128 runs; if it exceeds roughly a minute, reduce `steps` rather than the seed count — coverage of interleavings matters more than depth per seed.

- [ ] **Step 5: Run the crate suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/managed/simulation/scenario.rs \
        crates/morrow-runtime/src/managed/tests.rs
git commit -m "test(runtime): assert per-pair ordering, delivery and accounting under every interleaving"
```

---

### Task 11: Real scheduler threads

Everything above runs N schedulers deterministically on one thread. This task puts them on real threads. Because the semantics are already pinned by Tasks 9-10, a failure here is a threading failure and nothing else.

**Files:**
- Create: `crates/morrow-runtime/src/managed/threads.rs`
- Modify: `crates/morrow-runtime/src/managed/scheduler.rs:177-212` (`morrow_managed_run`)
- Modify: `crates/morrow-runtime/src/managed/host.rs` (host mode pins to one scheduler)
- Modify: `crates/morrow-runtime/src/lib.rs` (embedder-facing options)
- Test: `crates/morrow-runtime/src/managed/tests.rs`

**Interfaces:**
- Produces: `pub struct RuntimeOptions { pub schedulers: usize }`, `RuntimeOptions::default()` resolving `MORROW_SCHEDULERS` then `available_parallelism()`, `morrow_runtime::configure(options: RuntimeOptions)`; `threads::run(s: *mut Session, schedulers: usize)`.
- The `morrow_managed_run` signature is unchanged. This is a hard constraint, not a preference.

- [ ] **Step 1: Write the failing test**

Add to `crates/morrow-runtime/src/managed/tests.rs`:

```rust
#[test]
fn actors_execute_on_more_than_one_thread_simultaneously() {
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};
    // Observed thread ids, not a timing assertion: a run that merely *could* be
    // parallel but ran sequentially would pass a duration check on a fast machine.
    let threads: Arc<Mutex<BTreeSet<std::thread::ThreadId>>> = Arc::default();
    let observer = Arc::clone(&threads);
    let report = run_threaded_scenario_for_test(4, move || {
        observer.lock().unwrap().insert(std::thread::current().id());
    });
    assert_eq!(report.final_live, 0);
    assert!(
        threads.lock().unwrap().len() >= 2,
        "actor callbacks ran on {} distinct threads; parallel execution is not happening",
        threads.lock().unwrap().len()
    );
}

#[test]
fn run_returns_only_after_every_scheduler_has_quiesced() {
    let report = run_threaded_scenario_for_test(8, || {});
    assert_eq!(report.final_live, 0, "an actor outlived the run");
    assert_eq!(report.final_messages, 0, "a message outlived the run");
    assert_eq!(report.final_heap_bytes, 0, "a heap outlived the run");
}

#[test]
fn host_mode_runs_exactly_one_scheduler() {
    // morrow_managed_open pins to one scheduler: poll's contract and the HOSTS
    // thread-local both depend on it.
    let exec = open_host_for_test();
    assert_eq!(scheduler_count_for_test(exec), 1);
    close_host_for_test(exec);
}

#[test]
fn configuration_resolves_environment_then_available_parallelism() {
    assert_eq!(RuntimeOptions::resolve(Some("4")).schedulers, 4);
    assert_eq!(
        RuntimeOptions::resolve(Some("0")).schedulers,
        1,
        "a nonsense count falls back rather than producing zero schedulers"
    );
    assert!(RuntimeOptions::resolve(None).schedulers >= 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime more_than_one_thread`
Expected: FAIL — everything runs on the calling thread.

- [ ] **Step 3: Spawn the threads**

Create `crates/morrow-runtime/src/managed/threads.rs`. The calling thread becomes scheduler 0; workers 1..N are `std::thread::Builder` threads named `morrow-scheduler-{i}`. Each worker installs its own `Domain` cursor, runs the same loop body as scheduler 0, and parks on a condvar when `Quiesce::park` returns `Waiting`. `Parked::Terminated` broadcasts the condvar so every worker exits. Scheduler 0 joins all workers before returning from `morrow_managed_run`, preserving its contract.

A panic in a worker must not silently reduce the scheduler count: catch it, set the root fault, mark `stopped`, and broadcast so the other schedulers unwind through the existing `cleanup::unwind`/`finish` path.

Resolution order for the count: explicit `configure()` call, then `MORROW_SCHEDULERS`, then `available_parallelism()`, then 1. Host mode ignores all of it and uses 1.

- [ ] **Step 4: Run the focused tests**

Run: `cargo test -p morrow-runtime threaded host_mode configuration`
Expected: PASS, four tests.

- [ ] **Step 5: Run the crate suite repeatedly**

Run: `cargo test -p morrow-runtime && for i in 1 2 3 4 5; do cargo test -p morrow-runtime threaded || break; done`
Expected: PASS every time. A test that passes four times in five is a failing test — chase it before continuing rather than re-running until green.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/managed/threads.rs \
        crates/morrow-runtime/src/managed/scheduler.rs \
        crates/morrow-runtime/src/managed/host.rs \
        crates/morrow-runtime/src/lib.rs \
        crates/morrow-runtime/src/managed/tests.rs
git commit -m "feat(runtime): execute actors on N scheduler threads"
```

---

### Task 12: ThreadSanitizer over the threaded suite

Commit `8a8798c` added TSan over a suite that was single-threaded. Now it has something to find.

**Files:**
- Modify: `xtask/src/main.rs` (the TSan target added in `8a8798c`)
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Run TSan as it stands and read the output**

Run: `cargo xtask check` (or the TSan-only target `8a8798c` added)
Expected: this is a measurement, not a gate. Record what it reports. A clean first run on a newly threaded runtime deserves suspicion — confirm the threaded tests actually ran under it rather than being filtered out.

- [ ] **Step 2: Make the threaded scenarios part of the TSan target**

Ensure the TSan invocation runs the Task 11 tests with `MORROW_SCHEDULERS` set above 1, and repeats the threaded scenarios enough times to exercise interleavings. A single pass of a fast test is close to no coverage.

- [ ] **Step 3: Fix what it reports, then re-run**

Run: the TSan target
Expected: clean. **Do not suppress a report to reach green.** If a report is a genuine false positive, record why in the suppression file with a comment naming the exact synchronization that makes it safe.

- [ ] **Step 4: Commit**

```bash
git add xtask/src/main.rs .github/workflows/ci.yml
git commit -m "build: run the threaded runtime scenarios under ThreadSanitizer"
```

---

### Task 13: The scaling workload

The acceptance bar is measured parallel speedup, so this task builds the workload that measures it. The risk the spec records applies directly: a workload with no cross-scheduler sends measures thread spawning, not the design.

**Files:**
- Create: `benchmarks/language-comparison/programs/actor_scaling.mr`
- Modify: `benchmarks/README.md`
- Modify: `xtask/src/main.rs` (staging, if the existing benchmark commands need it)

- [ ] **Step 1: Write the workload**

A CPU-bound actor workload with deliberate cross-scheduler traffic. Workers are placed round-robin, so a coordinator dispatching to W workers and collecting their replies crosses schedulers in both directions. Each callback stays short and bounded, because without preemption a long callback blocks its scheduler and the curve would measure that instead.

Create `benchmarks/language-comparison/programs/actor_scaling.mr`. The syntax follows `crates/morrow/tests/actors/receive_suspends.mr` and `tail.mr`:

```morrow
# Runtime arguments: decimal worker count, decimal jobs per worker, decimal spin budget.
type Reply:
    Done(Int)

type Job:
    Work(Int, Pid(Reply))
    Stop

fn decimal(text: String, index: Int, value: Int) -> Int:
    if index == String.len(text): value
    else:
        match String.char_at(text, index):
            Some(digit) -> decimal(text, index: index + 1, value: value * 10 + digit - 48)
            None -> 0

# Bounded arithmetic per message: long enough to be worth scheduling, short
# enough that a cooperative scheduler is not blocked by one callback.
fn spin(remaining: Int, value: Int) -> Int:
    if remaining == 0: value
    else: spin(remaining - 1, (value * 48271) % 2147483647)

fn worker(budget: Int) -> ():
    receive:
        Work(seed, reply) ->
            match send(reply, Done(spin(budget, seed))):
                Ok(()) -> worker(budget)
                Err(_) -> ()
        Stop -> ()

fn workers(remaining: Int, budget: Int, built: List(Pid(Job))) -> List(Pid(Job)):
    if remaining == 0: built
    else: workers(remaining - 1, budget, List.push(built, spawn(() -> worker(budget))))

fn dispatch(targets: List(Pid(Job)), remaining: Int, seed: Int, me: Pid(Reply)) -> Int:
    if remaining == 0: 0
    else:
        let sent = List.fold(targets, 0, (count, target) ->
            match send(target, Work(seed + count, me)):
                Ok(()) -> count + 1
                Err(_) -> count)
        sent + dispatch(targets, remaining - 1, seed + 1, me)

fn collect(remaining: Int, total: Int) -> Int:
    if remaining == 0: total
    else:
        receive:
            Done(value) -> collect(remaining - 1, total + value)

fn main() -> ():
    let count = decimal(System.arg(1), index: 0, value: 0)
    let jobs = decimal(System.arg(2), index: 0, value: 0)
    let budget = decimal(System.arg(3), index: 0, value: 0)
    let targets = workers(count, budget, [])
    let me: Pid(Reply) = self()
    let sent = dispatch(targets, jobs, 1, me)
    println(collect(sent, 0))
```

Compile and run it before measuring anything: `cargo xtask build && ./bin/morrow run benchmarks/language-comparison/programs/actor_scaling.mr 8 64 512`. If `self()` is not the spelling this runtime uses for an actor's own PID, find the correct one in `crates/morrow/tests/actors/` rather than inventing one — the workload has to be an ordinary Morrow program, not a special case.

- [ ] **Step 2: Verify it produces cross-scheduler traffic**

Run it under the simulation driver at 1, 2, 4 and 8 schedulers and assert `cross_scheduler_sends > 0` and rising with scheduler count. A workload that does not cross schedulers cannot demonstrate anything about them.

- [ ] **Step 3: Measure the curve**

Follow the sampling method in `benchmarks/README.md` — interleaved samples per build, not batched, matching how `NATIVE_OPTIMIZATION.md` and `ARITHMETIC.md` were measured. Record 1, 2, 4 and 8 schedulers with `MORROW_SCHEDULERS`. Capture the numbers before claiming anything about them.

- [ ] **Step 4: Re-measure step 1's regression**

Run the immutable-model and scalar workloads that Decision156 recorded (59.03 → 59.96 ms, +1.58%, and +0.09%). Whether step 2 recovered that cost is a question this step answers with a number, not an expectation.

- [ ] **Step 5: Commit**

```bash
git add benchmarks/language-comparison/programs/actor_scaling.mr benchmarks/README.md xtask/src/main.rs
git commit -m "bench: add a cross-scheduler actor scaling workload"
```

---

### Task 14: The BEAM comparison and the evidence document

**Files:**
- Create: `benchmarks/language-comparison/SCHEDULERS.md`
- Create: the Elixir counterpart workload beside the existing BEAM programs

- [ ] **Step 1: Write the equivalent Elixir workload**

Same actor topology, same per-message arithmetic, same message count. Pin the Elixir/OTP versions and artifact hashes the way `BEAM.md` does.

- [ ] **Step 2: Measure both across scheduler counts**

Morrow with `MORROW_SCHEDULERS` at 1, 2, 4, 8; Elixir with `+S` at the same counts. Interleave samples between the two implementations, not batched by implementation.

- [ ] **Step 3: Write `SCHEDULERS.md`**

State the preemption asymmetry prominently and early: Morrow's scheduler is cooperative until step 3 and BEAM's is preemptive, so BEAM should win on fairness-sensitive shapes. The comparison is about scaling behavior, not a throughput verdict. Report the per-scheduler actor distribution — the supervised co-location risk means an unevenly distributed run is a real possible outcome and must be visible rather than averaged away.

Record what did not scale as plainly as what did.

- [ ] **Step 4: Commit**

```bash
git add benchmarks/language-comparison/SCHEDULERS.md benchmarks/language-comparison/programs/
git commit -m "bench: compare multi-scheduler actor scaling against BEAM"
```

---

### Task 15: Run the full gate, then record the decision

**Files:**
- Modify: `DECISIONS.md`, `ROADMAP.md`, `docs/ACTOR_RUNTIME.md`

- [ ] **Step 1: Run the full gate**

Run: `cargo xtask check`
Expected: PASS. This is the first full gate run since Task 1.

- [ ] **Step 2: Write Decision157**

Append to `DECISIONS.md` in the established format, numbered 157 (156 is sendable actor heaps):

- **Status**: ✅ Adopted
- **Decision**: each scheduler owns a `Domain`; `Pid` becomes an opaque `{slot, generation, mailbox}` resolved through a shared registry; cross-scheduler send and spawn transfer detached heap fragments adopted by a map merge; actors are placed round-robin at spawn and pinned for life, with supervised children co-located; scheduler count is runtime configuration; host mode runs one scheduler.
- **Context**: the measured scaling curve, the BEAM comparison with its preemption caveat, the re-measured step 1 regression, and the two findings this work produced — that `Heap`'s stable-address block map makes fragment adoption a merge rather than a relocation, and that step 1's "pinned to their spawning scheduler" would have produced no parallelism.
- **Consequences**: termination detection is now explicit global accounting; supervised co-location can defeat round-robin placement until step 4 adds migration; cooperative scheduling bounds achievable scaling until step 3 adds preemption.

- [ ] **Step 3: Update the roadmap and the actor runtime document**

Add the completed step to `ROADMAP.md` in the established `- [x]` form with its acceptance evidence: test counts, the scaling numbers, the TSan result, and the seed-sweep coverage. In `docs/ACTOR_RUNTIME.md`, correct the statement that the scheduler executes callbacks on one thread — step 1 left it accurate, and this step makes it false.

- [ ] **Step 4: Commit**

```bash
git add DECISIONS.md ROADMAP.md docs/ACTOR_RUNTIME.md
git commit -m "docs: record multi-scheduler execution and its measurements"
```

---
