# Actor runtime status and contracts

Fern executes typed native actors with the Rust runtime in
`crates/fern-runtime/src/managed`. The older string mailbox API in
`crates/fern-runtime/src/actors` remains a separate compatibility interface.
Its lifecycle events and strategies do not implicitly apply to typed actors.
The broader target language is recorded in [DESIGN.md](../DESIGN.md).

## Typed native execution

`spawn(initializer)` queues a typed, zero-argument initializer. Its captures are
copied into a new actor heap before it runs. Sending copies the validated message
graph into the receiving actor's heap, preserves sharing inside that graph, and
publishes nothing when validation or admission fails. Strings, aggregates,
full-width ranges and immutable `json.Value` graphs have owned representations;
PID references point only to invocation-owned control records. Unsupported
native handles cannot cross this boundary.

Each actor heap is collected independently and retired on actor completion or
failure. Sender collection and termination cannot invalidate a received payload.
Native compiler frames register typed reference roots. Runtime allocation helpers
still use conservative stack/register discovery, and objects are scanned
conservatively; this is not yet a fully precise collector.

The scheduler executes FIFO continuation callbacks on one thread. Supported
receives and receiving tail calls suspend into explicit continuation frames.
Ordinary native helpers, loops and recursion run synchronously inside their
callback: they have no instruction budget or preemption. There are no parallel
workers, cross-process PIDs or distributed scheduler guarantees.

## Typed failure and restart

`supervise(initializer, max_restarts)` returns `Pid(Message)` and retains a private
copy of the typed initializer. The restart count must be between 0 and 32 and is a
lifetime budget, not a rolling time window. A checked runtime failure unwinds the
generated function returns and active `defer` cleanup, retires the failed child's
payload heap and mailbox, then queues a fresh actor from the original initializer.
Unrelated actors remain runnable. Mutation of a failed child's captures does not
change the next initializer. Normal completion does not restart.

Each replacement has a fresh identity. `send(old_pid, message)` continues to
return an error; it never redirects. `supervised_current(original_pid)` returns
`Result(Pid(Message), Int)` for the currently live replacement, or an error when
the lineage has completed or exhausted its budget. Restart admission failure
ends that lineage without poisoning unrelated actors. Unsupervised actor faults
retain the invocation-failure behavior of ordinary native programs.

Recoverable checked faults include existing arithmetic/collection runtime
failures and the 16 MiB Regex replacement and terminal-rendering limits.
Generated source calls propagate the exact actor fault cell before using a
failed operation's result. This is not a recovery boundary for process aborts,
out-of-memory termination, foreign memory corruption or arbitrary external code.
Typed links, monitor messages, supervisor trees and restart strategies beyond a
single child are still future work.

## Native application hosts

Managed native libraries export descriptor-aware `fern_library_open(fault)` and
`fern_library_string_port(exec)` helpers. The opened invocation is explicitly
rooted until `fern_managed_close(exec)`. The host's writable fault cell must remain
at its original address until close, and all operations run on the opening thread.
Other retained native values need registered host root slots across calls.

`fern_managed_poll(exec, max_steps)` accepts 1–65,536 continuation callbacks and
returns 0 for completion, 1 for external-input idle, 2 for a reached callback
budget, or 3 for invocation failure. An idle persistent server is not a deadlock.
This count does not bound the synchronous work within a helper. String ports are
ordinary bounded, actor-owned mailboxes with host reads: peek reports the next
byte length; read copies UTF-8 into a host buffer and consumes exactly one message.
A short buffer returns -2 without consuming it; -1 means empty and -3 invalid.
Close cancels the invocation, retires actor heaps and drops its persistent root;
it does not collect or invalidate another open invocation.

Admission currently caps live actors at 1,024, mailbox messages at 4,096, total
queued messages at 65,536 and logical retained actor storage at 64 MiB. Actor
identities have a separate 65,536-per-invocation lifetime limit: dead control
records are retained to keep stale copied PID and supervision lineage references
safe. Reusing slots safely requires generation checks and explicit control-edge
ownership; it is not implemented yet. Repeated reads of an existing port or
lookups of a supervision lineage do not consume new actor identities.

## Compatibility mailbox behavior

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

## Compatibility lifecycle and supervision commitments

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

Typed runtime tests cover initializer isolation, independent heap reclamation,
atomic copying, stale PID rejection, fresh restart identity, bounded host reads,
repeated port reuse and independently rooted host sessions. Native executable
oracles compile real Fern programs and prove that supervised collection, Regex
and terminal-layout failures preserve sibling progress and execute active cleanup.

```sh
cargo test -p fern-runtime --lib
cargo test -p fern --test checker_actors --test lowering_actors --test cranelift_backend
```

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

The compatibility mailbox scheduler does not execute actor functions or
suspend/resume them. The typed native scheduler executes real actor functions
with isolated heaps and bounded single-child supervision, but generalized fair
suspension, safe identity-slot recycling, typed supervisor trees, synchronous
request/reply and REPL/FernSim parity remain open. Compatibility supervision
relationships form an acyclic hierarchy and supervisor death stops descendants.
Automatic ancestor escalation and descendant subtree recreation after supervisor
restart remain incomplete. Linked exits are notifications rather than full
bidirectional Erlang exit propagation. Neither contract promises parallel workers.

The compiler executes supported typed actor forms and diagnoses unsupported
ones. The mailbox and bounded execution tests establish their stated contracts;
they do not establish the complete actor model or the planned million-step
reliability target. See [ROADMAP.md](../ROADMAP.md) for remaining work and
[COMPATIBILITY_POLICY.md](COMPATIBILITY_POLICY.md) for project-wide guarantees.
