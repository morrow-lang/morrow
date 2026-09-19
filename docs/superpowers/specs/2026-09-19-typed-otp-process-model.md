# Typed process relationships and supervision

Status: stage 1 implemented and accepted; stages 2–5 planned.
Date: 2026-09-19.

## Goal and boundary

Extend the actor effort from measured performance to a coherent local process
model: isolated failures, links, independent monitors, typed lifecycle events,
and supervision trees with explicit restart and shutdown policies. This adds to
the performance work in `2026-09-19-actor-performance-parity.md`; it does not
retroactively broaden historical benchmark claims.

The target is the observable local core described below, not the entire OTP
ecosystem. Distribution, registered names, aliases, priority messages, process
dictionaries, hot code loading, `gen_server`, application controllers and
`simple_one_for_one` are separate work. Native callbacks remain cooperative.

## Existing implementation and compatibility boundary

The typed frontend currently checks `spawn`, `supervise`, `supervised_current`,
`send` and selective `receive` in `crates/morrow/src/check/actors.rs`. Actor
initializers are zero-argument Unit functions with a mailbox effect; a
`Pid(M)` accepts only messages of type `M`. Lowering lives under
`crates/morrow/src/lowering/actors/`.

The managed runtime already provides generation-stable PIDs, private heaps,
selective mailboxes, timeout continuations, owner-routed ingress and optional
migration. `managed/supervision.rs` provides a retained initializer and a
lifetime restart budget, capped at 32, for one lineage. Restart allocates a new
PID; `supervised_current` resolves the lineage without making old PIDs aliases.
Supervised actors currently remain pinned.

`crates/morrow-runtime/src/actors.rs` is a separate thread-local compatibility
registry with integer identities and string events. Its monitor registration
deduplicates observers, its restart window is anchored rather than rolling,
and its policies are stored on children. Its similarly named APIs are not an
implementation base or acceptance oracle for the typed process model.

An ordinary typed actor's fault is initially actor-local: `managed.rs::fail`
writes its fault cell. `scheduler.rs::step` and `wake_due` subsequently escalate
an unrecovered fault to the root. This permits an internal immutable failure
policy without changing the public `Exec` layout:

| Entry point | Child policy | Checked child fault |
| --- | --- | --- |
| Existing bare `spawn` | Legacy | Existing invocation failure |
| Existing `supervise` | Legacy lineage | Existing restart/exhaustion behavior |
| New `Process.spawn*` | Isolated | Retire this process and notify relationships |
| New supervisor child constructors | Isolated | Supervisor handles retirement |

Policy is selected by the called API, not silently inherited. Calling bare
`spawn` inside an isolated actor explicitly retains legacy behavior. Root/main
failure, invalid native ABI use and invocation infrastructure failure still stop
the invocation. Merely adding Process APIs cannot change an old program.
Mixed programs must understand that a legacy child's global failure can cancel
isolated processes too. No delivery guarantee survives invocation cancellation.

All callback, selector, timeout and cleanup fault paths must share one internal
retirement decision. Audit stop/drain paths separately: shutdown must preserve
the original global failure and must not restart children.

There is a second compatibility boundary: `scheduler.rs::idle` currently faults
the invocation with code 10 when no actor can run and no deadline exists. A live
isolated process is allowed to wait indefinitely. Track the invocation's live
isolated-process count as a liveness lease: while it is nonzero, idle means
parking for work or cancellation, not declaring a legacy deadlock. When it
reaches zero, existing legacy quiescence rules apply again. Cover both the local
driver and parallel idle barrier. A program using only old APIs retains its
deadlock behavior; a program creating a permanently waiting Process actor can
remain alive after main returns. Embedders need an owned thread-safe cancellation
token, not permission to call owner-only APIs through a borrowed `Exec`.

## Reference contract

The live official documentation reports OTP 29.1; implementation-level edge
cases below are checked against the benchmark baseline's OTP-29.0.6 tag.
The following compact reference is the external contract; later sections make
Morrow-specific representation and resource choices.

Links are symmetric and idempotent. Monitors are directional; each registration
gets an independent reference, including registration against a dead process
which produces `noproc`. Signals from one sender to one recipient preserve
order. Exit handling uses the recipient's trap flag when processed: ordinary
normal exits are ignored without trapping, other ordinary reasons terminate,
and trapping converts the signal into `EXIT`. Direct `kill` is untrappable and
becomes `killed`; a linked exit whose reason is `kill` follows ordinary link
rules. An inactive link suppresses its pending signal.
[Official process semantics](https://www.erlang.org/doc/system/ref_man_processes.html).

After demonitor returns, the monitor cannot place a new `DOWN` in the caller's
mailbox; an already queued event remains unless flushed. With `info`, the result
reports successful cancellation before delivery; with both `info` and `flush`,
it reports whether the monitor was still active, not whether a queued cell was
actually removed. After cancellation or consumption the result remains false. Unlink prevents subsequent effects
of that link on its caller, but does not erase an already queued `EXIT` or cancel
an explicit exit signal. The trap flag operation returns its previous value.
[Official BIF contracts](https://www.erlang.org/doc/apps/erts/erlang.html).

OTP 29's `exit_signal/2` separates signalling from local termination. The older
`exit(self(), normal)` has a special case that terminates an untrapping caller.
Morrow's two distinct APIs below follow `exit/1` and `exit_signal/2`, without
importing that older overload's self-normal quirk. Atomic spawn-with-monitor
and spawn-with-link close the child-exits-before-registration race.
[Pinned BIF definitions](https://raw.githubusercontent.com/erlang/otp/OTP-29.0.6/erts/preloaded/src/erlang.erl).

Supervisor strategy applies only when the terminated child's policy requests
restart. One-for-one affects that child; one-for-all affects all; rest-for-one
affects that child and later children in startup order. Termination is reverse
order and startup is forward order. Exceeding the supervisor's rolling restart
intensity terminates its children, then the supervisor with `shutdown`; defaults
are one restart in five seconds. Automatic significant-child shutdown applies
to natural child completion, not supervisor-induced termination.
[Official supervision principles](https://www.erlang.org/doc/system/sup_princ.html).

Permanent children restart after every termination; transient children exclude
normal and shutdown reasons; temporary children never restart and their specs
are removed. Numeric shutdown sends shutdown, waits, then kills; infinity waits
without that escalation. Manual terminate does not automatically restart a
child. Significant permanent children, and significant children with automatic
shutdown disabled, are invalid configurations.
[Official supervisor API](https://www.erlang.org/doc/apps/stdlib/supervisor.html).

The pinned implementation charges a restart attempt before applying a strategy,
not once per sibling created. Failed restart attempts retry through the event
loop and consume intensity. Its window uses monotonic whole seconds and keeps
timestamps equal to `now - period`. Temporary siblings are removed during group
termination; transient siblings can restart as part of a group's restart.
Shutdown waits using a monitor, so a child unlinking itself does not hang the
supervisor's death observation.
[Pinned supervisor implementation](https://raw.githubusercontent.com/erlang/otp/OTP-29.0.6/lib/stdlib/src/supervisor.erl).

The executable [monitor reference](../../process-model/monitor_reference.exs)
checks these edge cases on pinned OTP-29.0.6 at one, two and four schedulers.
The documentation's phrase about flushing being needed must not be interpreted
as “a mailbox cell was found”: the pinned
[`demonitor_2` implementation](https://raw.githubusercontent.com/erlang/otp/OTP-29.0.6/erts/emulator/beam/bif.c)
keeps the `info` result false when the monitor is absent, including after its
Down was already consumed. The same source creates no active self-monitor.

## Proposed source API

These are the full target signatures. Stage 1 implements spawn, spawn_monitor,
self, id, monitor, demonitor and receive_event; the remaining operations are
planned. See [the implemented API](../../PROCESS_MODEL.md).
`Entry(M)` below denotes the existing zero-argument Unit initializer with mailbox
effect `M`; it is not a new general first-class effect or existential facility.

```text
opaque ProcessId
opaque MonitorRef

Process.ExitReason = Normal | Shutdown | ShutdownDetail(String)
                   | Fault(Int) | Failure(String) | Kill | Killed | NoProcess
Process.Event(M) = Message(M)
                 | Down(MonitorRef, ProcessId, Process.ExitReason)
                 | Exit(ProcessId, Process.ExitReason)

Process.spawn(entry: Entry(M)) -> Result(Pid(M), Process.Error)
Process.spawn_link(entry: Entry(M)) -> Result(Pid(M), Process.Error)
Process.spawn_monitor(entry: Entry(M))
    -> Result((Pid(M), MonitorRef), Process.Error)
Process.self() -> Pid(M)
Process.id(pid: Pid(M)) -> ProcessId
Process.link(target: ProcessId) -> Result((), Process.Error)
Process.unlink(target: ProcessId) -> Result((), Process.Error)
Process.monitor(target: ProcessId) -> Result(MonitorRef, Process.Error)
Process.demonitor(ref: MonitorRef, options: DemonitorOptions)
    -> Result(Bool, Process.Error)
Process.trap_exit(enabled: Bool) -> Bool
Process.exit(reason: ExitReason) -> ()             # never resumes
Process.signal_exit(target: ProcessId, reason: ExitReason)
    -> Result((), Process.Error)
```

Only `spawn` and identity erasure are usable from the non-actor root context.
The other operations require an actual current actor. In particular the root
must spawn an observer actor to use `spawn_monitor`; do not invent a root mailbox
or retain a borrowed root `Exec`. Actor operations remain forbidden inside
receive guards and defer bodies. The compiler treats `Process.exit` as a terminal
actor operation rather than requiring a new public bottom type.

`ProcessId` erases only mailbox type, not identity or invocation scope. There is
no public conversion back to `Pid(M)` and no untyped send. It permits monitoring
heterogeneous workers without weakening typed messages. `MonitorRef` is fresh,
owner-stamped, comparable and copyable; copying it does not create another
monitor. A foreign owner cannot demonitor it. Both opaque values use validated
compiler descriptors and retained control tokens, never naked heap pointers.

New processes start with trapping disabled. Spawn-link and spawn-monitor publish
the relationship and immutable failure policy before making the child runnable;
failed admission publishes neither a child nor a partial relationship. The
caller need not run before a successfully created child exits.

Expected errors are tagged values: resource limit, foreign invocation, wrong
monitor owner, unsupported target such as a host port, and invalid options.
Morrow's existing `send -> Result` behavior remains unchanged. A valid dead local
identity can be monitored and linked: monitor succeeds with a fresh reference
and queued `NoProcess`; link produces the ordinary link-exit effect with that
reason. Signalling a dead valid identity is a successful no-op. Invalid foreign
identities fail before mutation. Self-link/unlink are no-ops. Self-monitor returns a fresh owner-stamped inert
reference without an active relationship or completion reservation; demonitor
with `info` returns false for it, matching the pinned runtime.

The reason vocabulary deliberately uses bounded typed data rather than arbitrary
Erlang terms. Stage 2 limits string reasons to 4,096 UTF-8 bytes, validated and
charged before a signal or local exit is admitted. Oversize signal reasons return
InvalidOptions; an oversize local terminal reason raises checked fault 9.
Malformed native reason representations remain infrastructure fault 11.
Normal return maps to `Normal`; checked actor
fault maps to `Fault(code)`. First accepted terminal cause wins. A cleanup failure
is retained as diagnostic data without replacing that cause. Panic/abort, OOM
and unsafe native corruption are not recoverable process exceptions.

### One mailbox, two receive views

```text
receive_event:
    Process.Message(message) -> handle(message)
    Process.Down(ref, process, reason) -> handle_down(ref, process, reason)
    Process.Exit(process, reason) -> handle_exit(process, reason)
    after 1000 -> handle_timeout()
```

`receive_event` checks arms against `Process.Event(M)` while the actor's send
type stays `M`. Existing `receive` matches only user-message cells and leaves
system-event cells in place. Both views use the same ordered mailbox and the
same selective receive cursor rules. Sending a user payload that happens to be
an Event value still creates `Message(payload)`, never a forged lifecycle cell.

Signals are processed at scheduler safe points independently of user receive.
An untrapped abnormal exit must retire an actor blocked on an unrelated pattern
or timeout. Process signal polling also occurs between callbacks inside a
multi-reduction turn. Trap-exit conversion and monitor delivery append cells at
their position in ordered ingress; they must not use a separately prioritized
user-visible event queue. A sender's accepted message followed by its exit
cannot produce that sender's `Down` ahead of the message. Different senders do
not acquire a specified global order; selective matching may skip older cells.

`DemonitorOptions` has `flush` and `info` booleans with the reference contract's
result meanings. In particular, `info + flush` returns false after prior
cancellation or Down consumption even when no cell remains to remove. Flush affects only this reference's system Down cell, never a
user payload with similar fields. Unlink uses a link-generation token so an old
in-flight exit cannot affect a newly established link. Once converted to an
Exit cell, unlink leaves it queued.

## Runtime ownership, admission and retirement

Add an invocation-owned relationship registry indexed by immutable process
identity. Records contain weak actor controls and retained immutable metadata;
they cannot retain another actor's mutable heap. Monitor ownership is explicit,
and link edges have epochs. Lookup, registration and the Alive-to-Exiting commit
are linearized so every registration/death race has one result. Exiting-to-Dead
occurs only after owner-thread retirement and the selected cleanup policy.

Use the existing activity publication protocol and route each signal through
the actor's stable ingress. Registry locks must not be held while acquiring
ingress locks, running cleanup, allocating a GC object or calling generated
code. Registration/retirement first commits immutable actions; bounded action
batches are then published under the normal activity fence. Cancellation tokens
bridge those phases. Unpublished committed actions count as outstanding work,
so quiescence cannot discard the last Down or link cascade. Stop must fence and
drain every admitted action, including ones between commit and publication.

Only the current owner mutates frame, heap, mailbox, trap state or cleanup.
Transfer must include pending signal state and policy and must preserve identity
and ordering. A receiver checks monitor/link tokens immediately before
materialization. Demonitor's caller-side cancellation invalidates future
materialization even if the sender already committed a notification. Recipient
death releases pending events, monitor ownership and links without strong cycles.
Cascade retirement uses bounded iterative batches, never recursive native stack
walks through an arbitrary link graph.

Relationship admission must reserve the future notification before success.
A full user mailbox cannot silently lose a Down, a trapped Exit or a supervisor
shutdown observation. Introduce explicit separate control-slot accounting within
the invocation's total memory budget: one monitor completion slot, two directed
completion slots for a link, and one slot per explicit admitted exit signal.
Move a reservation from relationship to pending signal to mailbox cell; release
it only on consumption, cancellation or recipient retirement. Repeated link
registration is free only while that same edge remains active. All failure
paths undo admission atomically. Bound edges, pending actions, reason bytes and
per-turn processing independently; finalize numeric defaults with quota tests
before stage 1 ships. Do not make ordinary actor spawn pay a large eager event
buffer allocation.

Automatic notifications need allocation-independent retirement: retain a single
immutable bounded reason record shared by control envelopes, and reserve its
maximum admitted storage before accepting a custom terminal cause. Built-in
fault/normal reasons require no fallible payload allocation. Receiving code may
materialize a typed copy on its own heap under ordinary fault policy. This is
necessary to avoid a full memory budget preventing the very notification that
would release a waiting supervisor.

Normal return, explicit local exit and checked failure drain admitted logical
defers once. A forced `Kill` skips user cleanup in the new process path while
still releasing runtime-owned roots, fragments and tokens exactly once. This is
an explicit new API contract, not a change to old invocation cancellation. A
blocked FFI call, synchronous builtin, finalizer or cleanup cannot be forcefully
interrupted safely. Deadline expiration requests Kill at the next safe point;
it does not authorize freeing an executing actor's heap or reporting it dead
early. Hard wall-clock kill guarantees require separate native isolation work.

## Typed supervisors

Keep the existing `supervise` implementation and lifetime budget API intact.
Implement a separate `Supervisor` namespace over the new process machinery.

```text
Strategy = OneForOne | OneForAll | RestForOne
Restart = Permanent | Transient | Temporary
Shutdown = Graceful(milliseconds: Int) | Infinity | Immediate
AutoShutdown = Never | AnySignificant | AllSignificant
Flags = { strategy, intensity: Int, period_seconds: Int, auto_shutdown }
ChildPolicy = { restart, shutdown, significant: Bool }
opaque Supervisor.Handle
opaque Supervisor.ChildKey(M)
opaque Supervisor.ChildSpec

Supervisor.child_key(name: String) -> ChildKey(M)
Supervisor.worker(key: ChildKey(M), entry: Entry(M), policy: ChildPolicy)
    -> Result(ChildSpec, Supervisor.Error)
Supervisor.branch(name: String, flags: Flags, children: List(ChildSpec), policy)
    -> Result(ChildSpec, Supervisor.Error)
Supervisor.start_link(flags: Flags, children: List(ChildSpec))
    -> Result(Supervisor.Handle, Supervisor.Error)
Supervisor.start(flags, children) -> Result(Supervisor.Handle, Supervisor.Error)
Supervisor.id(handle: Supervisor.Handle) -> ProcessId
Supervisor.current(handle, key: ChildKey(M)) -> Result(Pid(M), Supervisor.Error)
Supervisor.stop(handle) -> Result((), Supervisor.Error)
```

Keys carry an unforgeable spec token plus mailbox descriptor, so using the same
text with another type cannot reinterpret a child's PID. Names must be unique
inside one supervisor. `ChildSpec` is a compiler-recognized opaque package of
typed initializer descriptor and copied capture data; this limited erasure does
not add general existential functions. Templates live in the supervisor's
owned heap and are copied for each generation. Mutable worker state is never
used as its restart template. A branch is an actual isolated supervisor process,
not a metadata-only grouping. A handle does not expose a user-message PID.

Worker startup has an explicit acknowledgement protocol:

```text
Process.init_ack() -> Result((), Process.Error)
Process.init_ignore() -> ()                     # terminal startup outcome
Process.init_fail(reason: ExitReason) -> ()      # terminal startup outcome
```

These operations are valid only in the supervisor-created startup phase.
Returning before acknowledgement is failed initialization. A supervisor starts
children in spec order and waits for each acknowledgement before starting the
next. Its successful start result means all non-ignored children acknowledged;
it does not guarantee they remain alive afterward. A startup failure tears down
already-started children in reverse order before reporting failure. An isolated
plain spawn has no acknowledgement requirement. Start/stop/current requests
suspend actor continuations; they must not block an OS scheduler thread. An
unlinked root `Supervisor.start` adapter may drive the invocation, but root
`start_link` is invalid because the root is not a process.

Workers default to Permanent and Graceful(5000); branches default to Permanent
and Infinity. A graceful worker normally traps exits and returns or exits with
Shutdown after handling its Exit event. A supervisor always handles child exits
internally and serializes restart/stop operations. Its parent relation is
distinguished from ordinary child links; parent termination initiates supervisor
shutdown, including a normal parent termination, rather than being mistaken
for a child to restart. Explicit shutdown observation uses an internal monitor
independent of a child's ability to unlink.
[The parent-exit lifecycle is also described by the OTP server contract](https://www.erlang.org/doc/apps/stdlib/gen_server.html#c:terminate/2).

Use the reference restart rules, including rolling whole-second intensity and
group-attempt counting. Allow intensity zero; require positive bounded period.
The maximum configured intensity bounds timestamp storage. Startup attempts
before a supervisor has started are startup failure, not an automatic restart
storm. Once running, failed restart initialization yields before retry and is
charged again. Exceeding intensity retires the supervisor with Shutdown after
children have retired; a permanent parent may restart that supervisor.

Restart creates fresh identities. A monitor on the old child never follows its
replacement. `current` resolves the stable typed child key to the current
generation, and returns explicit stopped/restarting/removed states as errors.
There is no automatic mailbox forwarding. Supervisor-caused sibling shutdown
does not recursively trigger another strategy operation. Stale death events
are matched by both child key and generation.

After the static tree is accepted, add `start_child`, `terminate_child`,
`restart_child`, `delete_child` and `which_children`. Retained stopped specs,
temporary-spec removal and typed current lookup must have distinct states.
Implement significant-child automatic shutdown in that same final stage, using
the reference configuration validation and natural-versus-induced distinction.

## Module and ABI boundaries

| Area | Responsibility |
| --- | --- |
| `check/actors.rs` and dedicated `check/processes.rs` | Actor context, mailbox effects, opaque identity and Event typing |
| Parser/AST/IR actor modules | `receive_event` and terminal process operations |
| `lowering/actors/processes.rs` | New resumable operations and explicit context ABI calls |
| New `managed/process.rs` | Failure policy, reason/state machine, owner-safe retirement |
| New `managed/relations.rs` | Identity registry, monitor/link epochs and admission reservations |
| New `managed/signals.rs` | Bounded ordered signal processing and Event materialization |
| Existing `transport.rs`, `scheduler.rs`, `migration.rs` | Publication, safe points, wakeups, quiescence and transfer hooks |
| New `managed/supervisor/` | Child specs, startup protocol, strategies, restart window, shutdown state machine |
| Existing `managed/supervision.rs`, compatibility `actors.rs` | Preserve old APIs and tests |
| Descriptor/copy/cost modules | New opaque kinds and owned event payload validation |

Preserve the native `Exec`, `Type` and `Function` layouts, existing symbols and
callback signatures. Add new entry points and versioned side metadata if an
Event selector needs additional descriptor information. Never append fields to
an old descriptor and read beyond caller storage. New selector adapters present
the right typed Event shape without passing system cells to old selectors.
Native clients without new metadata retain their existing behavior. Internal
Actor fields can evolve, but new control roots must be included in GC scanning
and precise collection before transfer. Supervisor state stays owner-local and
pinned initially; relationships alone must not pin otherwise movable workers.

### Stage 1 implementation contract

Implementation review selects opaque descriptor leaves 13 (`ProcessId`) and 14
(`MonitorRef`), each with zero children. Existing `Exec`, `Type` and `Function`
layouts stay unchanged. Process IDs retain their actor identity; monitor
references retain an invocation epoch so allocator address reuse cannot make
references from different invocations equal. Pure equality helpers compare the
immutable identities, not wrapper addresses, including after a message copy.

Ordinary nominal tagged objects represent Event, ExitReason, Error and options.
Event tags are 0 Message, 1 Down, 2 Exit, with arities 1/3/2. ExitReason tags follow
the declaration order above (Normal through NoProcess, 0 through 7). Error tags
are 0 ResourceLimit, 1 ForeignInvocation, 2 WrongMonitorOwner, 3 UnsupportedTarget,
4 InvalidOptions. Result errors contain a boxed nullary Error. Spawn-monitor's
success is an ordinary tuple `[0, pid, reference]`.

Add `morrow_process_spawn(exec, entry, mailbox)`,
`morrow_process_spawn_monitor(exec, entry, mailbox)`,
`morrow_process_self(exec, mailbox)`, `morrow_process_id(exec, pid)`,
`morrow_process_monitor(exec, id)` and
`morrow_process_demonitor(exec, reference, flags)`. Flag bit 0 is flush and bit 1
is info; other bits are invalid. Add
`morrow_process_receive_event(exec, selector, timeout, duration, event_descriptor)`
with the existing callback status convention. Its selector receives a boxed
Event through the existing callback ABI, while `Function.mailbox` remains M.
The two equality helpers take only their two opaque values and need no Exec.

Initial control admission is 256 outstanding reservations per actor and 4,096
per invocation, at 512 logical bytes per monitor within the existing 64 MiB
budget. The observer's owner-only ledger retains the charge from registration
through pending/queued delivery; consumption, cancellation or observer retirement
releases it exactly once. Shared registry records use weak actor ownership and
must not form a reference cycle through invocation state. These limits are
subject to their exact-boundary and rollback tests before acceptance.

The implementation must update mailbox scan bounds and migration accounting for
both user and control cells. Cache each user-message Event wrapper after its
first materialization and account for its storage, avoiding repeated allocations
on an unmatched selective scan. Actor-only Process operations must seed mailbox
effect inference even without a receive. Event selectors must explicitly publish
Event(M)'s nominal layout even though the enclosing expression's result has a
different type. The REPL must reject unsupported process operations explicitly.

## Staged implementation and acceptance

Each stage starts with failing independent tests, then implementation, focused
native/frontend tests and the required full repository gate. Simulation tests
use injected time and barrier-controlled publication; sleeps are not race
oracles. Every accepted stage updates runtime docs, roadmap and decision record.

1. **Isolated processes and monitor delivery.** Add policy, typed identities,
   ordered system cells, `receive_event`, monitor/demonitor and atomic
   spawn-monitor. Keep all legacy fault fixtures unchanged. An isolated failing
   worker must produce exactly one Down while an unrelated worker finishes;
   ordinary receive must skip that Down without consuming it. Two monitors of
   one worker yield distinct refs and two events. Dead-before-monitor yields
   NoProcess. Barrier tests cover death/register, death/demonitor and flush both
   before and after materialization. A full user mailbox still admits an already
   reserved Down. Closing the invocation restores all reservations and physical
   allocations, including after a producer crossed the stop check.
   A final isolated waiter parks without fault 10, admitted ingress wakes it,
   cancellation retires it, and the last isolated retirement restores legacy
   quiescence decisions. Exercise the same transitions at the parallel idle
   barrier; pending relationship actions also keep the invocation alive.
2. **Links and exits.** Add link epochs, trap flag, local exit, direct signals
   and atomic spawn-link. Table tests cover Normal/Failure/Kill/Killed with
   trapping on/off and direct/linked origins. Verify self-signal separately
   from local exit. Repeated links remain one edge; unlink/relink suppresses an
   old pending linked signal while preserving an already queued Exit. A waiter
   with an unmatched receive and a computation-only actor both terminate on
   untrapped exit at safe points. Cyclic links retire iteratively with no leak.
   Force actor migration between user message and exit publication; assert
   message-before-Down and correct generation. Test cleanup-once versus forced
   Kill's cleanup suppression without freeing a blocked native callback.
3. **Static one-for-one supervisors.** Add opaque specs, startup acknowledgement,
   current lookup, restart types, rolling intensity and stop. Assert startup
   trace A-ready/B-ready/result, rollback reverse order on C failure, fresh
   generations and unchanged old PIDs. At intensity two, two failures can
   restart and the third inside the window shuts down; inject times exactly at
   and one second beyond the boundary. Charge failed restart attempts, not
   initial startup. Check intensity zero and invalid configuration atomically.
4. **Strategies and nested shutdown.** For A/B/C/D, fail B: one-for-one affects
   B; one-for-all stops D/C/A then starts A/B/C/D; rest-for-one stops D/C then
   starts B/C/D. Repeat with mixed policies: normal transient completion must
   not trigger sibling restarts, and temporary collateral children disappear.
   Verify one group attempt consumes one intensity slot. Child unlink cannot
   defeat shutdown observation. Test graceful success, deadline-to-Kill,
   Infinity, parent normal/abnormal exit and nested restart escalation. Do not
   assert a hard deadline while foreign code is executing.
5. **Dynamic child management and significant completion.** Verify stopped versus
   deleted specs, duplicate typed keys, temporary removal, generation-stale
   events and Any/AllSignificant. Manual or strategy-driven termination must
   not count as natural significant completion. Invalid significant/permanent
   or significant/Never combinations fail before children start.

For each stage, add fixed expected traces written independently of runtime
helpers. Small Erlang reference programs on pinned OTP-29.0.6 should record the
matching local behavior; normalize only identity values and allow explicitly
unordered independent-sender traces. Those comparisons supplement, rather than
replace, native ABI layout tests and fixed expected-output assertions. Record
the deliberate typed-reason, quota, cooperative-native and legacy-fault
differences alongside the traces. Exercise one and multiple schedulers with
stealing both disabled and enabled; retain existing migration/FIFO/stop tests.

## Readiness and remaining gaps

This document approves no claim of implemented OTP parity. Stage 1 implements
isolated processes, typed identities, monitors and event receive. Its native scheduler matrix,
ThreadSanitizer, deterministic replay and full repository gate pass (Decision165).
Stages 2–5 remain open. Resumable supervisor call lowering
needs implementation-level review in stage 3. The finite reason schema cannot
carry arbitrary Erlang terms. Existing host ports and TLS compatibility state
are not process targets. Existing native-resource pinning remains in force.
No acceptance stage claims native preemption or arbitrary OTP library support.

The original read-only investigation produced the proposal. Subsequent stage 1
implementation and reference fixtures are tracked independently from performance
checkpoints; neither is evidence that the other acceptance axis is complete.
