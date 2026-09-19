# Rust native actor execution

Status: bounded native implementation with actor-owned payload heaps and copied
messages, bounded single-child supervision and persistent native application
hosts. Typed native execution began with Decision105A; Decisions124–126 add the
ownership and application boundaries. Decisions138–142 add typed return frames,
logical cleanup, interactive execution and composable actor helpers. Typed
supervisor trees, instruction preemption and distributed execution remain open.

## Try the native example

Build the default compiler and run the [two-message example](../crates/morrow/tests/actors/receive_continues.mr):

```sh
cargo xtask build
./bin/morrow run crates/morrow/tests/actors/receive_continues.mr
```

It prints `one`, then `two`. The worker keeps its local state while waiting for the
second message; another actor sends that message after the worker suspends.
`spawn` returns an opaque `Pid(String)` for this worker. Each `send` returns a
Result that the example handles with `?` or `match`.

## Source contract

`supervise(entry, max_restarts)` takes a zero-argument Unit initializer and returns
`Pid(M)`. It retains an isolated initializer copy and allows 0–32 replacements
over the lineage's lifetime. A checked failure retires the child's heap and
mailbox, then queues a fresh identity from that copy. Normal completion does not
restart. Old PIDs remain dead for send; `supervised_current(original_pid)` returns
`Result(Pid(M), Int)` for the currently live replacement. Exhaustion or failed
restart admission ends the lineage without stopping unrelated actors. See the
[complete runtime contract](ACTOR_RUNTIME.md) for ownership and failure boundaries.

`spawn(entry)` takes a zero-argument function returning Unit and returns invariant opaque `Pid(M)`. Its captures are evaluated once; its body is queued, never run inline. `send(pid, message)` returns `Result((), Int)`: Ok means enqueue only; Err 3 means dead/foreign identity, Err 4 means mailbox/session quota or an unaccounted message graph. Send borrows its message and gives no Result-handling or transfer credit. Result-bearing messages, callable messages, and unaccounted native handles are rejected by checking. Result-bearing ordinary closure captures retain the existing prohibition. Compiler-created continuation frames may retain already-owned local Result duties; suspension is neither a completed exit nor handling credit, and every completed actor path must still satisfy the ordinary Result proof.

Mailbox schemes are inferred from owned receive patterns and body constraints,
then propagated across lexical direct calls. Recursive components are refined
before generalization, independent of declaration order; there is no arbitrary
scalar default. An indented `receive` selectively considers messages in enqueue order and arms in source order. Unmatched messages remain in order. Guards are bounded pure, nonallocating, nonfailing scalar expressions without calls. Duplicate or unreachable arms are rejected, but receive need not be exhaustive. An optional final `_ after duration -> body` evaluates duration once; it must be an Int in 0..600000 milliseconds. Registration first tries existing queued messages, even with zero duration. Subsequent polls consider only messages committed strictly before the absolute monotonic deadline. Timely messages do not lose because another actor delayed polling. At exact millisecond equality the deadline wins. Timeout fires only after no eligible message matches. Timer wake ordering is deadline, then stable actor identity. Timeout expressions and capture graphs are not reevaluated on wake.

Receiving helpers may return typed values, including Results. Direct calls retain
caller obligations through the ordinary source-function proof. Typed return
frames support non-tail calls, strict operands, recursive helpers, `for`, `with`
and `?`. Logical `defer` scopes survive suspension and drain on function return,
fault or cancellation. Only a spawned initializer must return Unit.

Ordinary captured and returned callbacks use separate resumable copies inside
actors; native non-actor calls keep their synchronous ABI. List and Option/Result
combinators preserve callback selection and evaluate arguments once. See the
[current continuation contract](ACTOR_CONTINUATIONS.md) for scheduling boundaries
and [actor cleanup](ACTOR_CLEANUP.md) for lifetimes and fault precedence. Blocking
native services, arbitrary instruction preemption and first-class actor-effect
helpers remain separate boundaries.

The [REPL scheduler](REPL_ACTORS.md) retains actors between entries, runs virtual
timeouts and exposes bounded source replay through MorrowSim. The old string-mailbox
API and its supervision policies remain a separate compatibility interface.

## Execution ABI and provenance

Ordinary generated ABI remains `(environment, fault, source arguments)` with an exactly 8-byte fault slot. Context-requiring direct entries use `(environment, fault, execution context, source arguments)`. Context is never captured into a source closure, stored in a global current-actor variable, or read from beyond the fault slot. First-class context-requiring ordinary helpers are rejected except an immediate spawn or supervise entry; pure first-class callbacks keep the ordinary ABI.

The native step callback is `int64_t(exec*, frame*)`; selectors are `void*(exec*, frame*, int64_t payload)`. Cranelift transports both native status and payload as 64-bit words. Immutable function descriptors bind exact code identity, capture count/types, callback kind, and mailbox type. Type descriptors have four 64-bit words: kind, count, child pointers, and sum arities. Public IR is validated before private CPS conversion; caller-created IR cannot construct the opaque private lowered operations. Original and inactive signatures, actor metadata, closure identities, and capture arity/types are checked before cloning. Unknown identities have no fallback.

A selected frame is allocated only after the complete pattern and guard succeed. Registration validates both selector and timeout identities before charging or publishing roots. Its new selector/timeout captures replace the spent entry root. Selection or timeout installs the successor before retiring old receive roots. Completion/cancellation clears frames, selectors, timeout state, messages, and queue links; dead identity metadata remains while a PID or supervision lineage retains it. Each actor owns a payload heap for its captures, frames and messages; invocation control data remains separately owned. Small bounded temporary graph indices use Rust-owned collections and are reclaimed on all paths.

Spawn and send copy supported message/capture graphs into receiver-owned storage,
preserving sharing within a copied graph. Collection cannot follow payloads into
another actor's heap. PID copies retain scheduler identity, without sharing the
other actor's payload. In-heap copy roots protect partial graphs; cross-scheduler copies own all their
blocks in off-heap fragments until owner adoption publishes a mailbox root. Actor callbacks enter their heap through a
scope guard; retirement releases the heap after active scopes finish. Compiler
root frames are explicit, but normal collection still uses conservative native
stack/register and heap-word scanning. See [runtime memory](MEMORY_MANAGEMENT.md).

## Ownership, quotas, and failures

One invocation owns immutable PID identities and one or more FIFO cooperative
schedulers. The default single scheduler runs source main before actor callbacks.
Opt-in parallel workers can run actors concurrently with main; successful main
drains the invocation, while main faults or returned Err cancel it. Set
`MORROW_SCHEDULERS` to 1–64 before invocation creation. Root spawns distribute
round-robin; actor-spawned children remain on the parent's scheduler. See the
[parallel execution contract](ACTOR_RUNTIME.md#typed-native-execution). An unsupervised actor fault stops the session after active ordinary
helper cleanup. A supervised child instead uses its bounded restart policy.
A waiting CLI session without runnable actors or a pending timer reports
deadlock. Blocking host calls and nonyielding source computation can delay
scheduling; work outside supported continuation boundaries has no preemption or
instruction budget.

Native application hosts can instead retain an explicitly rooted invocation
between calls. `morrow_managed_poll` advances a bounded number of continuation
callbacks and reports external-input idle without treating it as deadlock.
The host roots retained PID/value slots and keeps the borrowed fault cell stable
until close. Bounded String reply ports copy actor messages into host-owned
buffers; no actor payload pointer escapes through that interface. Closing one
invocation retires its heaps and persistent root without invalidating another.
The [host ABI contract](ACTOR_RUNTIME.md#native-application-hosts) specifies
statuses, quotas and thread ownership.

Elapsed timers resolve to ready successor frames at every cooperative scheduling
boundary, even while other actors remain runnable. Receivers already queued by
messages are reinserted once in deadline/identity order when overdue; timely
matching messages remain eligible. Promotion evaluates only validated pure
selectors and prepares continuations; it does not execute source actor bodies.
Later zero-duration receives cannot overtake an older promoted frame. Retiring the
cached earliest timer recomputes the minimum without an extra clock read.
Deterministic MorrowSim arrival traces exercise these rules through
the native scheduler, including timely unmatched messages and late arrivals.
This timer coverage does not establish generalized actor, supervision, or REPL
simulation parity.

Limits are 1024 live actors, 4096 messages per actor, 65536 queued messages globally, and 64 MiB aggregate logical retained ownership. Actor slots are reusable; nonwrapping `u64` generations replace the old 65536-lifetime identity limit. Exhausted generations fail admission without charging storage. Descriptor tables and value graph indices each contain at most 4096 entries, with 128 payload-depth limit. Descriptor registration shares 1,048,576 work units across identity and metadata inspection; each enqueue/frame graph attempt shares the same finite allowance across descriptor work, identity lookup, graph traversal, and 64-byte String scan units. Immutable DAGs share within one owner graph; separate enqueues are charged separately. Unknown native object graphs are not treated as scalar pointers. PID graphs must belong to the same session and exact mailbox identity.

An exited Actor control record is never overwritten when its slot is reused.
PID wrappers retain the exact actor generation; send also checks the live slot,
while supervision lookup follows the original immutable lineage. Foreign-heap
PID allocations and copies record one explicit control edge in allocation
metadata. Invocation collection visits those edges in time proportional to
allocated foreign blocks, without reading their payload bytes. Payload sweep or
heap retirement removes edges with their wrappers. The ordinary native collector
still scans stack/register roots; this edge protocol does not claim fully precise
collection for every native helper.

The logical quota accounts for active actor/supervisor headers and retained
payload graphs. Exiting releases the active header charge even if a stale PID
keeps dead control metadata physically reachable. Collector byte accounting
continues to include those allocations until their final roots disappear. The
logical limit is therefore not a hard bound on allocator bytes, metadata or
collection work.

Enqueue validates the graph and reserves bytes before copying into the receiver's heap. Its monotonic timestamp is read at commit after potentially expensive validation/allocation; a clock failure rolls back the reservation and leaves the mailbox unchanged. Failed sends never remove or reorder messages or publish partial copies. Receive validates duration, selector, timeout mailbox, and clock before publishing roots. Checked clocks reject invalid/overflowing time representations. GC storage is distinct from this logical retained quota: retiring a message root makes its graph collectible; retiring an actor releases its payload heap after active scopes exit. Actual host allocator exhaustion is not a recoverable logical-quota failure.

Fault 8: `actor timeout must be between 0 and 600000 milliseconds`.
Fault 9: `actor resource limit exceeded`.
Fault 10: `actor deadlock: no runnable actor or pending timeout`.
Fault 11: `invalid actor execution descriptor`.
Fault 12: `actor monotonic clock failure`.
Fault 13: `regex replacement exceeds 16 MiB`.
Fault 14: `terminal rendering exceeds 16 MiB`.

Existing fault codes 1..7 and their first-failure behavior remain unchanged.
Unhandled invocation failures use the ordinary `morrow: runtime error: ...`
diagnostic and exit 1 in native CLI programs. Supervised checked failures are
handled by the child's restart policy; no source Result type is silently
rewritten to encode execution faults. Allocation exhaustion, process aborts and
foreign memory corruption are outside this recoverable boundary.

## Validation scope

Native source oracles cover scheduling, full-width values, selective order,
zero/positive timeout, explicit helper contexts, Result duties held across
suspension, PID equality, cleanup, quotas and GC pressure. Rust runtime tests
exercise descriptor rejection, root retirement, cancellation, atomic failed send,
foreign identities, retained graph limits, timely/late delivery, timeout order
and clock failure rollback. Source/public-IR/REPL tests reject unsupported effects
before execution.

The native actor fixtures include 100,000 direct tail-continuation transitions
under one actor identity and collection pressure while a receiver retains live
values. Atomic rejection tests retain existing output files for unsupported
actor programs. Current runners are Rust and use the retained-identity supervisor
for bounded subprocess cleanup. The former C/MorrowSim/sanitizer totals remain
historical evidence; [workspace acceptance](RUST_WORKSPACE.md) identifies the
actual debug and optimized Rust checks.

Recursive Unit helper oracles independently assert bounded host-poll progress
for a sibling and 100,000 mutual tail calls under forced precise collection,
preserving String and full-width integer arguments. The same helpers remain
callable through the ordinary native ABI. Additional oracles preserve finite
helper scheduling, non-tail computation and cleanup, and reject forged control
types even in inactive code before private continuation conversion.

The ownership work adds independent cross-actor copy, root and heap-retirement
oracles. Supervision tests prove pristine initializer replay, fresh identities,
stale-send rejection and sibling progress after checked failures. Real compiled
Morrow tests cover active cleanup and collection, Regex and terminal-rendering
faults; persistent host tests cover independent session roots and repeated port
use. These results do not repurpose the historical migration totals.

The [web application](WEB_PREVIEW.md) now keeps each checklist room's state in a
compiled native Morrow actor through `morrow-web-app`; the Rust gateway receives
checked snapshot copies through a bounded reply port. Browser application logic
is compiled Morrow WebAssembly. This application evidence does not establish
generalized native actor fairness, typed supervisor trees or multicore scaling.
