+++
schema_version = 1
id = "01M2XHZ80APQ78N6KFW18606YR"
title = "Domain ownership is checked on tokens and on actor control records"
date = "2026-09-19"
status = "accepted"
tags = ["runtime", "infra"]
supersedes = []
superseded_by = []
depends_on = ["01M2XHZ80RZ3SCX1K8E08G2Q99"]
related_to = []
+++
## Status

Adopted for the token-identity contract; the remaining step 2 tasks are proposed in the [multi-scheduler design](../docs/superpowers/specs/2026-09-19-multi-scheduler-execution-design.md); verification is tracked in ROADMAP.md

## Decision

Every `Domain` carries a process-unique identity, handed out once and never reused. `Root` and `Scope` record the identity of the domain that issued them, and retiring either while a different domain is current is a hard error rather than a silent no-op. `Domain::shutdown` resets contents but preserves identity, so tokens a native caller still holds keep naming the same domain. Creating an actor payload heap whose control record is not an invocation-heap allocation is rejected by the same reasoning, matching the check `control_edge` already performed. This is the first task of step 2 toward parallel actor execution; it adds no parallelism.

## Context

[Decision 156](2026-09-15_192327128_move-heap-ownership-from-thread-local-storage-to-an-explicit.md) left `Domain::activate` and the `Activation` guard behind `#[allow(dead_code)]`, naming step 2 as their caller. Reading the tree for that step showed activation is not yet safe to use. Slot ids and root ids are per-domain counters that restart at the same values in every domain, and `Root::drop` routes through whichever domain is current, so a token registered under domain A and dropped under domain B removes *B's* registration with the same numbers — silently, because the map removal is a no-op on a missing key and a wrong unrooting on a colliding one. `Scope::drop` has the same shape and reached `leave`, whose existing assertion blamed scope ordering rather than the domain. This was never reachable in a shipped program, because nothing outside the tests calls `activate`; it becomes reachable the moment a scheduler activates a domain, which every other step 2 task requires.

## Evidence

Three regressions fail before the change and pass after. The root case did not panic at all, which is the silent corruption itself; the scope case panicked with "heap scopes must unwind in order", a diagnosis that blames the caller's nesting for a domain mismatch. A third case registers a control record allocated in a payload heap, which was accepted silently; a payload heap retiring underneath it would have left the new heap's external registration pointing at freed storage. A fourth, positive case pins that two domains issuing tokens with identical `(heap, id)` numbering stay independent, so the guards cannot be satisfied by over-rejecting. The seeded actor scenario is byte-identical before and after at two configurations: seed `0x0046524e` over 5,000 steps gives trace hash `0xd4e402a412f11e2f` with 48,739 callbacks, 1,246 delivered, 3,754 timeouts, 2,507 restarts and 10,000 churn actors; the default seed gives `0x9dae3d010549c7e7` with 48,567 callbacks. Both report zero live actors, messages, heap bytes and heap objects at cleanup. The runtime library suite grows from 107 to 111 tests. ThreadSanitizer runs the instrumented suite clean: zero race reports, 2,430.46 s on one Apple M4 over the 110 tests present when it ran, against [Decision 156](2026-09-15_192327128_move-heap-ownership-from-thread-local-storage-to-an-explicit.md)'s 107 tests in 2,389.85 s. The full macOS ARM64 `cargo xtask check` passes: 2,288 Rust tests across 290 suites, 316 native-output fixtures, 20 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192+231 fuzz cases.

## Recorded constraints

Reading the tree for step 2 produced five findings the design has to respect, and they are recorded here because rediscovering them late is expensive. Exactly four record kinds are allocated into the invocation heap — `Session`, its identity table, `Actor` and `Supervisor` — while PIDs, mailbox messages, cleanup scopes and copied frames belong to payload heaps; but the invocation heap is also where compiled `main` allocates ordinary program values, so it cannot become per-scheduler and those four records have to leave the collected heap instead. A stale PID must still reach a **retired** actor record to answer `morrow_managed_supervised_current` after its slot has been reused, which `managed/identity_tests.rs` asserts, so a global registry cannot replace the actor pointer — only its validation, by splitting an immutable identity header from scheduler-mutable state. `morrow_managed_send` copies the message into the receiver's heap, which across schedulers is a write into a heap another thread owns and may be collecting, so copy-at-send has to target an off-heap fragment the receiver adopts. The session's `live`, `messages`, `retained`, `next_id` and `used_slots` counters are a single 64 MiB accounting unit observable through `result_err(4)`, so splitting them per scheduler moves a user-visible limit and is a decision rather than an implementation detail. And the token hazard above.

## Consequences

A scheduler may now activate its domain without silently corrupting root registrations belonging to another. `Root` grows one word; the domain counter is a `Relaxed` atomic because domains are created on any thread and only their equality matters. A misplaced token aborts loudly instead of unrooting live data, which is the correct trade for a scheduler defect but does mean a native caller holding a token across an activation change will now see a panic where it previously saw silence. `Domain::activate` remains `#[allow(dead_code)]` until the owning-invocation task lands, because this task makes activation safe without yet giving it a caller. No observable Morrow behaviour, `extern "C"` signature or compiler symbol changes.
