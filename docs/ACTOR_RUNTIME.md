# Actor runtime status and contracts

This page describes the legacy C mailbox and supervision runtime. It does not
execute spawned Fern functions. The default Rust compiler separately supports
[bounded typed native actor execution](RUST_ACTORS.md); the two runtimes do not
yet share a complete supervision or FernSim execution model. For the explicit C
reference compiler, native build, run and IR emission reject `spawn`, `spawn_link` and
`receive` with an explicit diagnostic directing users to mailbox APIs.
`fern-c check` accepts their syntax and type signatures for tooling; a successful
check does not imply execution support. [DESIGN.md](../DESIGN.md) describes the
broader target language.

## Legacy C behavior

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
  restarting a dead owner does not reparent old children. Name-copy and replacement
  monitor-storage failures publish no live record or replacement ID. This baseline deliberately preserves monitors across
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

`tests/fixtures/runtime_actor_scenarios.c` links the actual runtime with FernSim,
not a separate implementation of supervision. The normal `mise run check` suite
runs it through `test_runtime_actor_seeded_lifecycle_invariants`:

- Zero-time restart-window exhaustion and exact window-boundary recovery.
- Single-use replacement lineage and permanent rejection of stale PIDs.
- Nested supervision registration, cycle rejection, and conflicting-owner rejection.
- Invalid PID handling and normally terminated siblings remaining stopped.
- Compiled `send` preserves both runtime errors and the successful `Ok(0)` payload.
- C native builds reject unsupported actor execution instead of generating placeholder
  worker/receive behavior.
- Eight reproducible seeds across all three strategies, with 64 crash steps each
  (1,536 total). Every step checks affected-child membership, notification counts,
  replacement IDs, empty replacement mailboxes, dead-PID rejection, and scheduler
  cleanup. Repeated replacement also exercises registry capacity growth.

For a focused replay after `mise run debug`:

```sh
cc -std=c11 -Wall -Wextra -Werror -Iruntime -Iinclude \
  tests/fixtures/runtime_actor_scenarios.c lib/fernsim.c lib/arena.c \
  bin/libfern_runtime.a $(pkg-config --libs bdw-gc sqlite3 openssl) \
  -pthread -o /tmp/fern-actor-scenarios
FERN_ACTOR_SCENARIO=simulation /tmp/fern-actor-scenarios
```

Other scenario names are `time-zero`, `single-replacement`, `forest`,
`invalid-pid`, and `terminated-sibling`. A simulation failure prints the seed and
strategy so the same case can be reproduced.

The additional `scripts/test_runtime_actor_subtree.py` gate compiles the actual
actor implementation in debug, release and AddressSanitizer/UndefinedBehaviorSanitizer
modes. Ten groups cover normal/abnormal/shutdown trees, registration order independent
of PID order, notification allocation/send failures, already-stopped branches,
unrelated current context, strategy-driven descendant shutdown, 2,048-level trees,
dead-owner restarts and atomic name/monitor allocation failures. The same builds
rerun all six prior scenarios, including their 1,536 seeded crash steps. Both C and
Rust quality gates run this suite; it is verified on macOS and Linux arm64.

## Work still required before concurrency is ready for applications

The legacy scheduler does not execute actor functions or suspend/resume them.
The default Rust scheduler provides those capabilities within the [105A limits](RUST_ACTORS.md),
but generalized suspension, isolated per-actor heaps, synchronous request/reply,
typed supervision and REPL/FernSim parity remain open. Legacy supervision
relationships form an acyclic hierarchy and supervisor death stops descendants.
Automatic ancestor escalation and descendant subtree recreation after supervisor
restart remain incomplete. Linked exits are notifications rather than full
bidirectional Erlang exit propagation. Neither contract promises parallel workers.

The C compiler rejects `spawn(worker)`; the Rust frontend executes supported
forms and diagnoses unsupported ones. The legacy tests establish mailbox and
supervision-policy behavior. Together with the bounded Rust execution tests,
they still do not establish the complete actor model or the planned million-step
reliability target.
See [ROADMAP.md](../ROADMAP.md) for the remaining milestone work and
[COMPATIBILITY_POLICY.md](COMPATIBILITY_POLICY.md) for project-wide guarantees.

## Rust native execution

The default Rust compiler has a separate [typed native actor contract](RUST_ACTORS.md)
with cooperative execution, selective receive, monotonic deadlines and bounded
continuations. The C mailbox/supervision APIs above retain their current behavior.
Generalized suspension, typed supervision and REPL/FernSim parity remain open.
