# Sendable Actor Heaps Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move actor heap ownership out of thread-local storage into an explicit `Domain` value that can be constructed, moved between threads and owned by a scheduler, without changing any observable behavior.

**Architecture:** `memory/heaps.rs` currently keeps all heap state in `thread_local! { static STORE: RefCell<Store> }`. This plan renames `Store` to `Domain`, converts its free functions into `&mut self` methods, and reduces the thread-local to a cursor pointing at the domain currently executing on this thread. Foreign entry point signatures do not change, so no codegen or ABI work is required.

**Tech Stack:** Rust (pinned nightly via `rust-toolchain.toml`), Cargo workspace, `cargo xtask` for gates, ThreadSanitizer via `-Zsanitizer=thread`.

**Spec:** `docs/superpowers/specs/2026-09-15-sendable-actor-heaps-design.md`

## Global Constraints

- No observable behavior change. Public Morrow semantics, `extern "C"` signatures and the compiler's symbol contract in `crates/morrow/src/runtime.rs` and `runtime_abi.rs` are unchanged.
- No parallelism in this step. No second scheduler, no stealing, no migration, no preemption.
- Message payloads are copied without exception. No shared payload representation may be introduced here.
- Do not remove, skip or weaken a failing oracle to make a gate pass (CLAUDE.md).
- `Root` and `Scope` tokens keep `PhantomData<Rc<()>>` and stay `!Send`. They are per-thread stack tokens; only `Domain` and `Heap` become movable.
- Run `cargo xtask check` before the final commit of the last task, not after every task.
- Tests live in `crates/morrow-runtime/src/memory/tests.rs`, included via `#[cfg(test)] #[path = "memory/tests.rs"] mod tests;` at `memory.rs:24`, so they may use private items directly.

**Practical note:** there is no `target/` directory in this checkout. The first `cargo test -p morrow-runtime` is a cold build of the workspace and will take several minutes. Budget for it once, at Task 1 Step 2.

---

## File Structure

| File | Responsibility | Change |
| --- | --- | --- |
| `crates/morrow-runtime/src/memory/heaps.rs` | Domain type, slots, frames, scopes, collection entry | Heavily modified |
| `crates/morrow-runtime/src/memory.rs` | `Heap`, `Block`, `Root`, public allocation API | Lightly modified (Task 6 only) |
| `crates/morrow-runtime/src/memory/tests.rs` | Unit tests for the above | Extended in every task |
| `crates/morrow-runtime/src/managed/simulation/scenario.rs` | Seeded scenarios | Extended in Task 8 |
| `xtask/src/lib.rs` or `xtask/src/main.rs` | Gate commands | Extended in Task 9 |
| `.github/workflows/ci.yml` | CI gates | Extended in Task 9 |
| `DECISIONS.md`, `ROADMAP.md`, `docs/ACTOR_RUNTIME.md` | Record | Task 10 |

Tasks 1-5 are a mechanical extraction: each moves one group of free functions into `impl Domain` and leaves a thin delegating wrapper behind, so the rest of the crate keeps compiling untouched. Task 6 removes the thread-local's ownership. Nothing before Task 6 changes ownership, which is what makes the extraction reviewable.

**Out of scope, deliberately:** `crates/morrow-runtime/src/actors/api.rs:3` holds a second thread-local, `STATE`, for the compatibility mailbox scheduler. `docs/ACTOR_RUNTIME.md:199` records that this scheduler "does not execute actor functions or suspend/resume them". It is not on the parallel execution path and is left alone in this step.

---

### Task 1: Rename `Store` to `Domain` and extract the allocation path

**Files:**
- Modify: `crates/morrow-runtime/src/memory/heaps.rs:23-38` (struct), `:66-79` (thread-local and accessors)
- Test: `crates/morrow-runtime/src/memory/tests.rs`

**Interfaces:**
- Produces: `pub(crate) struct Domain`, `Domain::new() -> Domain`, `Domain::with<R>(&self, f: impl FnOnce(&Heap) -> R) -> R`, `Domain::with_mut<R>(&mut self, f: impl FnOnce(&mut Heap) -> R) -> R`. The free functions `heaps::with` and `heaps::with_mut` keep their exact current signatures and delegate.

- [ ] **Step 1: Write the failing test**

Append to `crates/morrow-runtime/src/memory/tests.rs`:

```rust
#[test]
fn two_domains_allocate_into_independent_heaps() {
    let mut first = Domain::new();
    let mut second = Domain::new();
    let a = first.with_mut(|heap| heap.allocate(64, false));
    let b = second.with_mut(|heap| heap.allocate(32, false));
    assert!(!a.is_null() && !b.is_null());
    assert_eq!(first.with(|heap| heap.bytes), 64);
    assert_eq!(second.with(|heap| heap.bytes), 32);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime two_domains_allocate_into_independent_heaps`
Expected: FAIL, `cannot find type Domain in this scope`. This is the cold build; expect several minutes.

- [ ] **Step 3: Rename the struct and add the constructor**

In `heaps.rs`, rename `struct Store` to `pub(crate) struct Domain` and rename its `impl Store` block to `impl Domain`. Change `fn new()` to `pub(crate) fn new()`. Update the thread-local at `heaps.rs:66` to:

```rust
thread_local! { static STORE: RefCell<Domain> = RefCell::new(Domain::new()); }
```

- [ ] **Step 4: Add the two accessor methods and delegate**

Add to `impl Domain`:

```rust
    pub(crate) fn with<R>(&self, f: impl FnOnce(&Heap) -> R) -> R {
        f(&self.slots[&self.active].heap)
    }
    pub(crate) fn with_mut<R>(&mut self, f: impl FnOnce(&mut Heap) -> R) -> R {
        let active = self.active;
        f(&mut self.slots.get_mut(&active).unwrap().heap)
    }
```

Replace the free functions at `heaps.rs:68-79` with delegating wrappers:

```rust
pub(super) fn with<R>(f: impl FnOnce(&Heap) -> R) -> R {
    STORE.with(|store| store.borrow().with(f))
}
pub(super) fn with_mut<R>(f: impl FnOnce(&mut Heap) -> R) -> R {
    STORE.with(|store| store.borrow_mut().with_mut(f))
}
```

- [ ] **Step 5: Run the test and the crate suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS, including the new test and every pre-existing test.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/memory/heaps.rs crates/morrow-runtime/src/memory/tests.rs
git commit -m "refactor(runtime): name the heap store a Domain and extract its allocation path"
```

---

### Task 2: Extract root registration onto `Domain`

**Files:**
- Modify: `crates/morrow-runtime/src/memory/heaps.rs:144-172` (`register`, `root`, `remove_root`)
- Test: `crates/morrow-runtime/src/memory/tests.rs`

**Interfaces:**
- Consumes: `Domain` from Task 1.
- Produces: `Domain::root(&mut self, pointer: *const usize, words: usize) -> Root`, `Domain::remove_root(&mut self, heap: usize, id: usize)`. Free functions `heaps::root` and `heaps::remove_root` keep their signatures.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn domain_roots_retain_only_their_own_heap() {
    let mut domain = Domain::new();
    let mut slot = 0usize;
    let block = domain.with_mut(|heap| heap.allocate(16, false));
    slot = block as usize;
    let root = domain.root(&slot as *const usize, 1);
    assert_eq!(domain.collect_active(&[]).objects, 1);
    drop(root);
    assert_eq!(domain.collect_active(&[]).objects, 0);
}
```

Note: `collect_active` arrives in Task 5. Until then this test will not compile, so write it now and leave it failing — Step 2 records that failure, and Task 5 Step 5 is where it turns green. If you prefer a green suite between tasks, gate it with `#[ignore]` and remove the attribute in Task 5.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime domain_roots_retain_only_their_own_heap`
Expected: FAIL, `no method named collect_active`.

- [ ] **Step 3: Move `register` and add the methods**

`register` is already a free function taking `&mut Heap` at `heaps.rs:144`; leave it as is. Add to `impl Domain`:

```rust
    pub(crate) fn root(&mut self, pointer: *const usize, words: usize) -> Root {
        let active = self.active;
        let id = register(&mut self.slots.get_mut(&active).unwrap().heap, pointer, words);
        Root { heap: active, id, _thread: PhantomData }
    }
    pub(crate) fn remove_root(&mut self, heap: usize, id: usize) {
        if let Some(slot) = self.slots.get_mut(&heap) {
            slot.heap.roots.remove(&id);
        }
    }
```

- [ ] **Step 4: Delegate the free functions**

```rust
pub(super) fn root(pointer: *const usize, words: usize) -> Root {
    STORE.with(|store| store.borrow_mut().root(pointer, words))
}
pub(super) fn remove_root(heap: usize, id: usize) {
    let _ = STORE.try_with(|store| store.borrow_mut().remove_root(heap, id));
}
```

`try_with` must be retained: `Root::drop` can run during thread-local destruction, when `with` would panic.

- [ ] **Step 5: Run the suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS except `domain_roots_retain_only_their_own_heap`, which still fails on `collect_active`.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/memory/heaps.rs crates/morrow-runtime/src/memory/tests.rs
git commit -m "refactor(runtime): extract root registration onto Domain"
```

---

### Task 3: Extract native frame registration onto `Domain`

**Files:**
- Modify: `crates/morrow-runtime/src/memory/heaps.rs:317-360` (`morrow_gc_frame_enter`, `morrow_gc_frame_leave`)
- Test: `crates/morrow-runtime/src/memory/tests.rs`

**Interfaces:**
- Produces: `Domain::frame_enter(&mut self, slots: *const usize, words: usize) -> usize`, `Domain::frame_leave(&mut self, token: usize)`. The two `extern "C"` symbols keep their exact signatures and delegate.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn domain_frames_are_scoped_to_their_domain() {
    let mut first = Domain::new();
    let mut second = Domain::new();
    let words = [0usize; 4];
    let token = first.frame_enter(words.as_ptr(), 4);
    assert_eq!(token, 1);
    assert_eq!(second.frame_enter(words.as_ptr(), 4), 1);
    first.frame_leave(token);
    assert_eq!(first.frame_count(), 0);
    assert_eq!(second.frame_count(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime domain_frames_are_scoped_to_their_domain`
Expected: FAIL, `no method named frame_enter`.

- [ ] **Step 3: Add the methods**

Add to `impl Domain`, moving the bodies verbatim from the two `extern "C"` functions:

```rust
    pub(crate) fn frame_enter(&mut self, slots: *const usize, words: usize) -> usize {
        let heap = self.active;
        self.next_frame = self.next_frame.checked_add(1).unwrap_or_else(|| std::process::abort());
        let token = self.next_frame;
        self.frames.push(Frame { token, heap, pointer: slots as usize, words });
        token
    }
    pub(crate) fn frame_leave(&mut self, token: usize) {
        if self.frames.last().is_some_and(|frame| frame.token == token) {
            // Avoid even a zero-length memmove on the ordinary callback exit.
            self.frames.pop();
        } else if let Some(index) = self.frames.iter().rposition(|frame| frame.token == token) {
            // Unusual cross-heap/out-of-order exits retain the remaining order.
            self.frames.remove(index);
        }
    }
    #[cfg(test)]
    pub(crate) fn frame_count(&self) -> usize {
        self.frames.len()
    }
```

- [ ] **Step 4: Delegate the exported symbols**

Keep the assertions and the safety documentation exactly as they are; replace only the bodies:

```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_gc_frame_enter(slots: *const usize, words: usize) -> usize {
    assert!(!slots.is_null() || words == 0);
    assert!(words <= 1_048_576);
    STORE.with(|store| store.borrow_mut().frame_enter(slots, words))
}
#[unsafe(no_mangle)]
pub extern "C" fn morrow_gc_frame_leave(token: usize) {
    STORE.with(|store| store.borrow_mut().frame_leave(token));
}
```

- [ ] **Step 5: Run the suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS, including the pre-existing `root_and_native_frame_tokens_retire_their_original_heap_after_scope_switch` at `tests.rs:149`, which is the regression guard for this task.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/memory/heaps.rs crates/morrow-runtime/src/memory/tests.rs
git commit -m "refactor(runtime): extract native frame registration onto Domain"
```

---

### Task 4: Extract actor heap lifecycle onto `Domain`

**Files:**
- Modify: `crates/morrow-runtime/src/memory/heaps.rs:50-64` (`remove_retired`), `:81-108` (`control_edge`), `:178-215` (`Scope`, `enter`), `:216-262` (`create`, `retire`), `:263-276` (`owns`)
- Test: `crates/morrow-runtime/src/memory/tests.rs`

**Interfaces:**
- Produces: `Domain::create_actor_heap(&mut self, control: *const usize, words: usize) -> usize` (unsafe), `Domain::retire_heap(&mut self, id: usize)`, `Domain::owns(&self, id: usize, pointer: *const c_void) -> bool`, `Domain::control_edge(&mut self, pointer: *const u8, control: *const u8)` (unsafe). `Scope` and `enter` stay thread-local-bound and are handled in Task 6, because `Scope::drop` needs the cursor.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn actor_heaps_are_created_and_retired_within_one_domain() {
    let mut domain = Domain::new();
    let control = domain.with_mut(|heap| heap.allocate(8, false));
    // SAFETY: control is an invocation-heap allocation of one word, live for this test.
    let id = unsafe { domain.create_actor_heap(control.cast::<usize>(), 1) };
    assert_ne!(id, 0);
    assert!(!domain.owns(id, control.cast()));
    domain.retire_heap(id);
    assert!(!domain.owns(id, control.cast()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime actor_heaps_are_created_and_retired_within_one_domain`
Expected: FAIL, `no method named create_actor_heap`.

- [ ] **Step 3: Move the four bodies onto `Domain`**

Move the bodies of `create`, `retire`, `owns` and `control_edge` into `impl Domain`, replacing each `STORE.with(|store| { let mut store = store.borrow_mut(); ... })` wrapper with direct `self` field access. `remove_retired` is already an `impl` method and needs no change. Preserve every assertion and safety comment verbatim, in particular the `expect("PID allocation belongs to current heap")` in `control_edge` and the `if id == 0 { return; }` guard in `retire`.

- [ ] **Step 4: Delegate the free functions**

```rust
pub(crate) unsafe fn create(control: *const usize, words: usize) -> usize {
    STORE.with(|store| unsafe { store.borrow_mut().create_actor_heap(control, words) })
}
pub(crate) fn retire(id: usize) {
    STORE.with(|store| store.borrow_mut().retire_heap(id));
}
pub(crate) fn owns(id: usize, pointer: *const std::ffi::c_void) -> bool {
    STORE.with(|store| store.borrow().owns(id, pointer))
}
pub(crate) unsafe fn control_edge(pointer: *const u8, control: *const u8) {
    STORE.with(|store| unsafe { store.borrow_mut().control_edge(pointer, control) });
}
```

- [ ] **Step 5: Run the suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/memory/heaps.rs crates/morrow-runtime/src/memory/tests.rs
git commit -m "refactor(runtime): extract actor heap lifecycle onto Domain"
```

---

### Task 5: Extract collection, stats and shutdown onto `Domain`

**Files:**
- Modify: `crates/morrow-runtime/src/memory/heaps.rs:110-143` (`collect`), `:277-296` (`stats`), `:297-316` (`shutdown`)
- Test: `crates/morrow-runtime/src/memory/tests.rs`

**Interfaces:**
- Produces: `Domain::collect_active(&mut self, roots: &[usize]) -> Stats`, `Domain::stats(&self) -> Stats`, `Domain::shutdown(&mut self)`.

This is the task where the spec's collection-time coupling becomes concrete: when `self.active == 0`, `collect` walks **every** non-zero slot's blocks to gather control words. That loop is why a domain must own all of its actor heaps rather than lending them out, and it is the constraint step 2's scheduler design has to respect.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn invocation_collection_scans_control_words_of_every_actor_heap_in_the_domain() {
    let mut domain = Domain::new();
    let control = domain.with_mut(|heap| heap.allocate(8, false));
    // SAFETY: control is a live one-word invocation allocation owned by this domain.
    let id = unsafe { domain.create_actor_heap(control.cast::<usize>(), 1) };
    let before = domain.stats();
    assert!(before.objects >= 1);
    let retained = domain.collect_active(&[]);
    assert!(
        retained.objects >= 1,
        "the actor control word must survive collection of the invocation heap"
    );
    domain.retire_heap(id);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime invocation_collection_scans_control_words`
Expected: FAIL, `no method named collect_active`.

- [ ] **Step 3: Move the three bodies onto `Domain`**

Move `collect` into `impl Domain` as `collect_active`, replacing `store.` with `self.` throughout and keeping the `active == 0` control-gathering branch and both comments verbatim. Move `stats` and `shutdown` the same way. `shutdown` ends with `*store = Store::new();`, which becomes `*self = Domain::new();`.

- [ ] **Step 4: Delegate the free functions**

```rust
pub(super) fn collect(roots: &[usize]) -> Stats {
    STORE.with(|store| store.borrow_mut().collect_active(roots))
}
pub(super) fn stats() -> Stats {
    STORE.with(|store| store.borrow().stats())
}
pub(super) fn shutdown() {
    STORE.with(|store| store.borrow_mut().shutdown());
}
```

- [ ] **Step 5: Remove the `#[ignore]` from Task 2's test and run the suite**

If `domain_roots_retain_only_their_own_heap` was gated with `#[ignore]` in Task 2, remove the attribute now.

Run: `cargo test -p morrow-runtime`
Expected: PASS, all tests including both previously failing ones.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/memory/heaps.rs crates/morrow-runtime/src/memory/tests.rs
git commit -m "refactor(runtime): extract collection, stats and shutdown onto Domain"
```

---

### Task 6: Reduce the thread-local to a cursor

**Files:**
- Modify: `crates/morrow-runtime/src/memory/heaps.rs:66` (thread-local), `:178-215` (`Scope`, `enter`), and every delegating wrapper written in Tasks 1-5
- Test: `crates/morrow-runtime/src/memory/tests.rs`

**Interfaces:**
- Produces: `Domain::activate(&mut self) -> Activation<'_>`, an RAII guard that sets the thread cursor on construction and restores the previous value on drop. `heaps::with_current<R>(f: impl FnOnce(&mut Domain) -> R) -> R`, which resolves the cursor, falling back to this thread's default domain.

This is the only task that changes ownership. Everything before it was extraction.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_domain_can_be_built_on_one_thread_and_used_on_another() {
    let mut domain = Domain::new();
    let bytes = domain.with_mut(|heap| heap.allocate(48, false));
    assert!(!bytes.is_null());
    let moved = std::thread::spawn(move || {
        let mut domain = domain;
        {
            let _active = domain.activate();
            // Allocating through the ordinary public path must land in this domain.
            let more = crate::memory::alloc(16, false);
            assert!(!more.is_null());
        }
        domain.with(|heap| heap.bytes)
    })
    .join()
    .expect("moved domain thread");
    assert_eq!(moved, 64);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime a_domain_can_be_built_on_one_thread_and_used_on_another`
Expected: FAIL. Either `no method named activate`, or a `Send` error naming `Domain` if any field still carries a `!Send` marker.

- [ ] **Step 3: Add the cursor, the default domain and the guard**

Replace the thread-local at `heaps.rs:66` with:

```rust
thread_local! {
    // Owned fallback for programs and tests that never activate a domain explicitly.
    static DEFAULT: RefCell<Domain> = RefCell::new(Domain::new());
    // Borrowed cursor: never owns a domain, always points at one that outlives it.
    static CURRENT: Cell<*mut Domain> = const { Cell::new(std::ptr::null_mut()) };
}

/// Run `f` against the domain currently executing on this thread.
pub(super) fn with_current<R>(f: impl FnOnce(&mut Domain) -> R) -> R {
    let current = CURRENT.with(|cell| cell.get());
    if current.is_null() {
        DEFAULT.with(|domain| f(&mut domain.borrow_mut()))
    } else {
        // SAFETY: the pointer was installed by Activation, which restores the previous
        // value on drop and cannot outlive the &mut Domain it borrowed.
        f(unsafe { &mut *current })
    }
}

/// Installs `domain` as this thread's current domain until dropped.
pub(crate) struct Activation<'a> {
    previous: *mut Domain,
    _domain: PhantomData<&'a mut Domain>,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Activation<'_> {
    fn drop(&mut self) {
        CURRENT.with(|cell| cell.set(self.previous));
    }
}
impl Domain {
    pub(crate) fn activate(&mut self) -> Activation<'_> {
        let previous = CURRENT.with(|cell| cell.replace(self as *mut Domain));
        Activation { previous, _domain: PhantomData, _thread: PhantomData }
    }
}
```

Add `use std::cell::Cell;` to the imports if `super::*` does not already provide it.

- [ ] **Step 4: Repoint every wrapper at the cursor**

Every wrapper written in Tasks 1-5 changes from `STORE.with(|store| store.borrow_mut().method(..))` to `with_current(|domain| domain.method(..))`. The one exception is `remove_root`, which must tolerate thread-local destruction:

```rust
pub(super) fn remove_root(heap: usize, id: usize) {
    let current = CURRENT.with(|cell| cell.get());
    if !current.is_null() {
        // SAFETY: as in with_current; Activation guarantees the pointer is live.
        unsafe { &mut *current }.remove_root(heap, id);
        return;
    }
    let _ = DEFAULT.try_with(|domain| domain.borrow_mut().remove_root(heap, id));
}
```

`Scope` and `enter` also move here. `enter` becomes `with_current(|domain| ...)` and `Scope::drop` likewise, keeping the `assert_eq!(store.active, self.entered, "heap scopes must unwind in order")` assertion verbatim. `Scope` keeps its `_thread: PhantomData<Rc<()>>`.

- [ ] **Step 5: Run the suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS, including the new cross-thread test and every pre-existing test.

- [ ] **Step 6: Commit**

```bash
git add crates/morrow-runtime/src/memory/heaps.rs crates/morrow-runtime/src/memory/tests.rs
git commit -m "feat(runtime): make the heap thread-local a cursor over an owned Domain"
```

---

### Task 7: Assert the ownership properties in the type system

**Files:**
- Modify: `crates/morrow-runtime/src/memory/heaps.rs` (end of file)
- Test: `crates/morrow-runtime/src/memory/tests.rs`

**Interfaces:**
- Produces: a compile-time assertion that `Domain: Send`. No runtime surface.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn domain_is_send_and_tokens_are_not() {
    fn assert_send<T: Send>() {}
    assert_send::<Domain>();
    // Root and Scope intentionally stay thread-bound; see the design document.
    // A negative bound cannot be asserted in stable Rust, so this test documents
    // the positive half and the PhantomData markers enforce the negative half.
    assert_eq!(
        std::mem::size_of::<crate::memory::Root>(),
        std::mem::size_of::<usize>() * 2,
        "Root carries two ids and a zero-sized thread marker"
    );
}
```

- [ ] **Step 2: Run test to verify it fails or passes**

Run: `cargo test -p morrow-runtime domain_is_send_and_tokens_are_not`
Expected: PASS if Tasks 1-6 left no `!Send` field on `Domain`; FAIL with a `Send` bound error naming the offending field otherwise. If it fails, the offending field is the finding — fix it before continuing and note it in the commit message.

- [ ] **Step 3: Add the permanent compile-time guard**

At the end of `heaps.rs`:

```rust
// A Domain must remain movable between threads: step 2 hands one to each scheduler.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Domain>();
};
```

- [ ] **Step 4: Run the suite**

Run: `cargo test -p morrow-runtime`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/morrow-runtime/src/memory/heaps.rs crates/morrow-runtime/src/memory/tests.rs
git commit -m "test(runtime): assert Domain stays Send and heap tokens stay thread-bound"
```

---

### Task 8: Classify and assert every cross-heap edge

**Files:**
- Modify: `crates/morrow-runtime/src/memory/heaps.rs`
- Modify: `crates/morrow-runtime/src/managed/simulation/scenario.rs`
- Test: `crates/morrow-runtime/src/memory/tests.rs`

**Interfaces:**
- Produces: `Domain::verify_edges(&self) -> Result<(), EdgeViolation>` and `pub(crate) struct EdgeViolation { pub heap: usize, pub block: usize, pub target: usize }`.

This is the spec's central deliverable. Step 4 of the overall sequence has to sever cross-domain references; this task establishes exactly which ones exist.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn only_control_edges_leave_an_actor_payload_heap() {
    let mut domain = Domain::new();
    let control = domain.with_mut(|heap| heap.allocate(8, false));
    // SAFETY: one live invocation-heap word, retired at the end of this test.
    let id = unsafe { domain.create_actor_heap(control.cast::<usize>(), 1) };
    {
        let _scope = domain.enter_scope(id);
        domain.with_mut(|heap| heap.allocate(32, false));
    }
    assert_eq!(domain.verify_edges(), Ok(()));
    domain.retire_heap(id);
}
```

`enter_scope` is `Domain`'s method form of `enter`, added in Task 6 Step 4.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p morrow-runtime only_control_edges_leave_an_actor_payload_heap`
Expected: FAIL, `no method named verify_edges`.

- [ ] **Step 3: Implement the classification**

```rust
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EdgeViolation {
    pub heap: usize,
    pub block: usize,
    pub target: usize,
}

impl Domain {
    /// Assert that the only edge leaving an actor payload heap is its control edge.
    ///
    /// Three classes are permitted, and step 4 of the parallel scheduler work has to
    /// sever exactly the second one:
    ///   1. payload to payload within one heap,
    ///   2. payload to control storage in the invocation heap,
    ///   3. invocation-internal.
    pub(crate) fn verify_edges(&self) -> Result<(), EdgeViolation> {
        let invocation: Vec<usize> = self.slots[&0].heap.blocks.keys().copied().collect();
        for (&id, slot) in &self.slots {
            if id == 0 {
                continue;
            }
            for (&base, block) in &slot.heap.blocks {
                if block.control != 0 && !invocation.contains(&block.control) {
                    return Err(EdgeViolation { heap: id, block: base, target: block.control });
                }
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p morrow-runtime only_control_edges_leave_an_actor_payload_heap`
Expected: PASS.

- [ ] **Step 5: Call it from the seeded scenarios**

In `crates/morrow-runtime/src/managed/simulation/scenario.rs`, at the point where each scenario step completes, add:

```rust
        #[cfg(debug_assertions)]
        if let Err(violation) = domain.verify_edges() {
            panic!("cross-heap edge violation during simulation: {violation:?}");
        }
```

Place it beside the existing per-step accounting so it runs on every one of the scenario's steps. If the scenario code has no `Domain` in scope, reach it through `heaps::with_current(|domain| domain.verify_edges())`.

- [ ] **Step 6: Run the seeded scenarios**

Run: `cargo xtask simulate --actors --seed 42 --steps 5000`
Expected: PASS with no violation panic. Then run the recorded evidence scenario and confirm exact replay:

```bash
cargo xtask simulate --seed 42 --steps 3000 --days 30 --json > /tmp/scenario.json
cargo xtask simulate --replay /tmp/scenario.json
```

Expected: replay succeeds. Any divergence is a defect introduced by Tasks 1-7, since none of them changed scheduling.

- [ ] **Step 7: Commit**

```bash
git add crates/morrow-runtime/src/memory/heaps.rs crates/morrow-runtime/src/memory/tests.rs crates/morrow-runtime/src/managed/simulation/scenario.rs
git commit -m "test(runtime): classify and assert every cross-heap edge in seeded scenarios"
```

---

### Task 9: Add ThreadSanitizer to the gate

**Files:**
- Modify: `xtask/src/main.rs` (command dispatch)
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: `cargo xtask tsan`, which runs the runtime test suite under ThreadSanitizer.

TSan finds nothing while execution is single-threaded. It is introduced now, while it is quiet, so the harness and suppressions exist before step 2 makes it load-bearing.

- [ ] **Step 1: Add the xtask command**

`xtask/src/main.rs:43` dispatches with `match command { "name" if guard => ..., }`. Add a `"tsan" if rest.is_empty() =>` arm alongside the existing `"fuzz"` arm at `:166`, and add a matching line to the help text at `:44`. The command runs:

```rust
Command::new(cargo)
    .args(["test", "-p", "morrow-runtime", "--target", host_target])
    .env("RUSTFLAGS", "-Zsanitizer=thread")
    .env("RUSTDOCFLAGS", "-Zsanitizer=thread")
```

ThreadSanitizer requires an explicit `--target`, so pass the host triple rather than relying on the default.

- [ ] **Step 2: Run it**

Run: `cargo xtask tsan`
Expected: PASS with no data race reports. A cold sanitizer build recompiles the dependency graph; expect several minutes.

- [ ] **Step 3: Add the CI job**

Add a job to `.github/workflows/ci.yml` mirroring the existing "Check compiler, runtime, tooling and native programs" job, running `cargo xtask tsan`. Restrict it to Linux ARM64, matching the platforms `docs/RUST_WORKSPACE.md` records as verified.

- [ ] **Step 4: Commit**

```bash
git add xtask/src/main.rs .github/workflows/ci.yml
git commit -m "build: run the runtime suite under ThreadSanitizer in the gate"
```

---

### Task 10: Measure, then record the decision

**Files:**
- Modify: `DECISIONS.md`, `ROADMAP.md`, `docs/ACTOR_RUNTIME.md`

- [ ] **Step 1: Measure allocation throughput against the recorded baselines**

Run the immutable-model and arithmetic workloads that `benchmarks/language-comparison/NATIVE_OPTIMIZATION.md` and `ARITHMETIC.md` record, following the commands in `benchmarks/README.md`. Capture the numbers before claiming anything.

Expected: no regression. Tasks 1-5 replaced a `RefCell` borrow plus a `BTreeMap` lookup with the same lookup behind a pointer; Task 6 replaced the `thread_local` lookup with a `Cell` read. If a workload regresses measurably, that is a finding to record, not to round off.

- [ ] **Step 2: Run the full gate**

Run: `cargo xtask check`
Expected: PASS. This is the first full gate run of the plan.

- [ ] **Step 3: Write Decision156**

Append to `DECISIONS.md` in the established format, numbered 156 (155 is the Morrow rename):

- **Status**: ✅ Adopted
- **Decision**: heap ownership moves from thread-local storage to an explicit `Domain`; the thread-local becomes a scheduling cursor; message payloads remain copied without exception; determinism becomes a property of the simulation driver rather than of the runtime.
- **Context**: state what was measured in Step 1, and record the two findings this work produced — that per-actor heap isolation already existed behind an `active` cursor, and that invocation-heap collection scans every actor heap in the domain, which constrains step 2's scheduler design.
- **Consequences**: `Domain: Send`; `Root` and `Scope` stay thread-bound; TSan joins the gate; step 2 may hand one `Domain` per scheduler.

- [ ] **Step 4: Update the roadmap and the actor runtime document**

Add the completed step to `ROADMAP.md` in the established `- [x]` form with its acceptance evidence. In `docs/ACTOR_RUNTIME.md`, leave the single-thread statement at line 25 accurate — it is still true — and note that heap ownership no longer requires it.

- [ ] **Step 5: Commit**

```bash
git add DECISIONS.md ROADMAP.md docs/ACTOR_RUNTIME.md
git commit -m "docs: record sendable actor heaps and their measurements"
```

---

## Self-Review

**Spec coverage.** Sendable heaps: Tasks 1-7. Explicit `Domain` owning invocation heap, frames and accounting: Tasks 1-5. Thread-local reduced to a cursor with an RAII guard covering unwinding: Task 6. Cross-heap edge enumeration and assertion: Task 8. Gate items 1-4 from the spec: Task 8 Step 6 (existing gate and exact replay), Task 8 Step 5 (edge classification), Task 9 (TSan). Gate item 5, before-and-after measurements: Task 10 Step 1. The spec's "risks" section on the ambient cursor is addressed by the `Activation` guard in Task 6 Step 3.

**Deviation from the spec, recorded here deliberately.** The spec says all 257 `no_mangle` entry points resolve their heap through the ambient store. That is true but misleading about the work: they reach it through roughly twelve functions in `memory.rs` (`:217-286`), so this plan changes no entry point signatures and no call sites outside `memory/heaps.rs`. The spec's estimate of this step was too pessimistic, and the plan is correspondingly smaller.

**Second deviation.** The spec treats control edges as the crux. Task 5 surfaces a second, sharper coupling the spec missed: `collect` scans every actor heap in the domain when the invocation heap is active. It is called out in Task 5 and folded into Decision156's context in Task 10.
