# Actor runtime status and contracts

This page describes the Rust mailbox and supervision runtime in
`crates/fern-runtime/src/actors`. These mailbox APIs do not themselves execute
spawned Fern functions. [Typed native actors](RUST_ACTORS.md) use a separate
bounded cooperative scheduler; the two interfaces do not yet share a complete
typed supervision or FernSim execution model. The broader target language is
recorded in [DESIGN.md](../DESIGN.md).

## Mailbox behavior

`actors.start(name)` creates a process-local integer ID and an empty mailbox.
`actors.post(pid, message)` and `send(pid, message)` copy a string into its FIFO mailbox
and return the runtime Result directly (`Ok(0)` or an error);
`actors.next(pid)` removes the oldest string. An empty mailbox returns
`Err(FERN_ERR_IO)` immediately. The C ABI also exposes round-robin scheduler
tickets: each successful send supplies one ticket, and requesting a ticket does
not itself execute code or consume the message.

The runtime C ABI has explicit current-actor context, lifecycle transitions,
virtual clock controls, and exit injection for integration and simulation tests.
`spawn_link` requires a live current actor to have been set with
`fern_actor_set_current`; creating an actor record alone does not set that context.
`actors.monitor`, `actors.demonitor`, `actors.restart`, `actors.supervise`,
`actors.supervise_one_for_all`, and `actors.supervise_rest_for_one` have checker,
codegen, and runtime implementations.

## Lifecycle and supervision commitments

- Exited PIDs are permanently dead. They cannot receive new messages, participate
  in scheduling, or become current. Invalid IDs, including `INT64_MAX` at the C
  ABI boundary, return errors rather than triggering assertions.
- Restart creates a new, empty mailbox with a new ID. Each dead PID can acquire
  only one replacement. Trying to restart the original PID again returns an
  error, even if the replacement has subsequently died. Restart the latest PID.
- Monitor registrations, linked-parent identity, and the child's own supervision
  policy survive restart. Its original supervising owner must still be alive;
  restarting a dead owner does not reparent old children. This baseline preserves monitors across
  replacements; it does not implement Erlang monitor-reference semantics.
- A supervised child has one owner. Registration rejects self-supervision, cycles,
  and changing the owner to a different supervisor. Rejected registration leaves
  the existing relationships intact. Re-registering with the same owner updates
  policy and resets that child's restart budget.
- Exiting a supervisor stops its owned descendant subtree before any notification
  can allocate or fail. The root retains its reason; newly stopped descendants use
  `shutdown`. Current-actor context and scheduler tickets are cleared for all of
  them. External live links/monitors receive preorder notifications in child
  registration order; dead observers inside the subtree receive none. Already-dead
  descendants are not notified again. The first notification error is returned,
  with no rollback; later notifications and automatic restarts are not guaranteed.
- `normal` and `shutdown` exits deliver notifications but do not automatically
  restart. An already-dead sibling stays dead during another child's strategy
  restart. Explicitly restarting that stopped child remains available while its owner is alive.
- Abnormal exits apply `one_for_one`, `one_for_all`, or `rest_for_one` to children
  registered with the same strategy. `rest_for_one` uses registration order.
  Affected live siblings stop with `shutdown` before replacements are created in
  registration order. Existing messages are not replayed into replacements.
- Restart intensity is currently **per child**, measured in a fixed window that
  begins with its first failure. Time zero is a valid start, and the window resets
  when elapsed seconds reach the configured period. Tests use an explicit clock;
  ordinary runtime operation uses system time. Exhaustion leaves the failed PID
  dead, emits `ESCALATE(pid,reason)`, and returns an error.
- Links receive `Exit(pid,reason)`; monitors receive `DOWN(pid,reason)`; successful
  automatic replacements generate `RESTART(old_pid,new_pid)` to the supervisor.
  These are strings in the current mailbox ABI, not typed message variants.

## Regression coverage

Rust runtime tests exercise FIFO messages and round-robin tickets, forest
cycle/owner rejection, stale identities and single-use restart lineage,
zero-time restart windows, both sibling restart strategies, and descendant
notification order. They call the actual runtime implementation.

```sh
cargo test -p fern-runtime actors::tests
cargo test -p fern-runtime --release actors::tests
cargo xtask native actors/
```

The retired C/FernSim harness and sanitizer totals describe earlier
implementation evidence. The [workspace acceptance](RUST_WORKSPACE.md) records
verification of the Rust runtime; those earlier totals are not silently reused.

## Work still required before concurrency is ready for applications

The mailbox scheduler does not execute actor functions or suspend/resume them.
The typed native scheduler provides those capabilities within the [105A limits](RUST_ACTORS.md),
but generalized suspension, isolated per-actor heaps, synchronous request/reply,
typed supervision and REPL/FernSim parity remain open. Legacy supervision
relationships form an acyclic hierarchy and supervisor death stops descendants.
Automatic ancestor escalation and descendant subtree recreation after supervisor
restart remain incomplete. Linked exits are notifications rather than full
bidirectional Erlang exit propagation. Neither contract promises parallel workers.

The compiler executes supported typed actor forms and diagnoses unsupported
ones. The mailbox and bounded execution tests establish their stated contracts;
they do not establish the complete actor model or the planned million-step
reliability target. See [ROADMAP.md](../ROADMAP.md) for remaining work and
[COMPATIBILITY_POLICY.md](COMPATIBILITY_POLICY.md) for project-wide guarantees.
