# Rust native actor execution

Status: bounded native implementation with actor-owned payload heaps and copied
messages, bounded single-child supervision and persistent native application
hosts. Typed native execution began with Decision105A; Decisions124–126 add the
ownership and application boundaries. Generalized fair suspension, typed
supervisor trees, multicore execution and complete deterministic FernSim parity
remain later stages.

## Try the native example

Build the default compiler and run the [two-message example](../crates/fern/tests/actors/receive_continues.fn):

```sh
cargo xtask build
./bin/fern run crates/fern/tests/actors/receive_continues.fn
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

Mailbox schemes are inferred from the owned receive patterns, with no arbitrary scalar default; source body constraints do not supply additional mailbox inference in this checkpoint. An indented `receive` selectively considers messages in enqueue order and arms in source order. Unmatched messages remain in order. Guards are bounded pure, nonallocating, nonfailing scalar expressions without calls. Duplicate or unreachable arms are rejected, but receive need not be exhaustive. An optional final `_ after duration -> body` evaluates duration once; it must be an Int in 0..600000 milliseconds. Registration first tries existing queued messages, even with zero duration. Subsequent polls consider only messages committed strictly before the absolute monotonic deadline. Timely messages do not lose because another actor delayed polling. At exact millisecond equality the deadline wins. Timeout fires only after no eligible message matches. Timer wake ordering is deadline, then stable actor identity. Timeout expressions and capture graphs are not reevaluated on wake.

Receiving functions return Unit and may suspend in tail position, block statements/initializers, If/Match branches, and explicit returns. Direct receiving calls in tail position update a continuation frame. Receiving-call Result arguments currently retain their caller duties; an otherwise valid callee-based discharge may be conservatively rejected until receiving-call summaries are proved. Non-tail receiving calls, receive inside For/With or strict operands, arbitrary indirect receiving calls, and receiving functions owning defer are diagnosed as unsupported. Ordinary pure spawned functions retain ordinary function-exit defer behavior. Calls into ordinary helpers retain their normal cleanup behavior. An actor suspension never runs defer.

Statically known actor entries can also suspend direct Unit tail-call paths that
lead to recursive helper cycles. The compiler retains ordinary callable entries
and emits separate actor continuations; each recursive handoff returns from the
native stack and queues its typed argument frame. This subset requires owned
capture/parameter types and excludes bodies with `defer`, `for` or `with`.
Ordinary calls, finite helper paths, non-tail calls, numeric-result recursion and
indirect calls preserve synchronous behavior. This is cooperative recursion
support, not an instruction budget for arbitrary source computation.

The REPL rejects 105A actor programs before effects or retained definitions change. Mailbox actor APIs and their supervision policies remain separate; complete FernSim parity is not claimed.

## Execution ABI and provenance

Ordinary generated ABI remains `(environment, fault, source arguments)` with an exactly 8-byte fault slot. Context-requiring direct entries use `(environment, fault, execution context, source arguments)`. Context is never captured into a source closure, stored in a global current-actor variable, or read from beyond the fault slot. First-class context-requiring ordinary helpers are rejected except an immediate spawn or supervise entry; pure first-class callbacks keep the ordinary ABI.

The native step callback is `int64_t(exec*, frame*)`; selectors are `void*(exec*, frame*, int64_t payload)`. Cranelift transports both native status and payload as 64-bit words. Immutable function descriptors bind exact code identity, capture count/types, callback kind, and mailbox type. Type descriptors have four 64-bit words: kind, count, child pointers, and sum arities. Public IR is validated before private CPS conversion; caller-created IR cannot construct the opaque private lowered operations. Original and inactive signatures, actor metadata, closure identities, and capture arity/types are checked before cloning. Unknown identities have no fallback.

A selected frame is allocated only after the complete pattern and guard succeed. Registration validates both selector and timeout identities before charging or publishing roots. Its new selector/timeout captures replace the spent entry root. Selection or timeout installs the successor before retiring old receive roots. Completion/cancellation clears frames, selectors, timeout state, messages, and queue links; dead identity metadata remains while a PID or supervision lineage retains it. Each actor owns a payload heap for its captures, frames and messages; invocation control data remains separately owned. Small bounded temporary graph indices use Rust-owned collections and are reclaimed on all paths.

Spawn and send copy supported message/capture graphs into receiver-owned storage,
preserving sharing within a copied graph. Collection cannot follow payloads into
another actor's heap. PID copies retain scheduler identity, without sharing the
other actor's payload. Runtime copy roots protect partial graphs until their
frame or mailbox root is published. Actor callbacks enter their heap through a
scope guard; retirement releases the heap after active scopes finish. Compiler
root frames are explicit, but normal collection still uses conservative native
stack/register and heap-word scanning. See [runtime memory](MEMORY_MANAGEMENT.md).

## Ownership, quotas, and failures

One invocation owns a FIFO cooperative scheduler, immutable PID identities and
mailboxes. In native CLI programs, main executes first. Successful main drains
actors; main faults or returned Err stop pending actors without running their
bodies. An unsupervised actor fault stops the session after active ordinary
helper cleanup. A supervised child instead uses its bounded restart policy.
A waiting CLI session without runnable actors or a pending timer reports
deadlock. Blocking host calls and nonyielding source computation can delay
scheduling; work outside supported continuation boundaries has no preemption or
instruction budget.

Native application hosts can instead retain an explicitly rooted invocation
between calls. `fern_managed_poll` advances a bounded number of continuation
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
Deterministic FernSim arrival traces exercise these rules through
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
Unhandled invocation failures use the ordinary `fern: runtime error: ...`
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
for bounded subprocess cleanup. The former C/FernSim/sanitizer totals remain
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
Fern tests cover active cleanup and collection, Regex and terminal-rendering
faults; persistent host tests cover independent session roots and repeated port
use. These results do not repurpose the historical migration totals.

The [web application](WEB_PREVIEW.md) now keeps each checklist room's state in a
compiled native Fern actor through `fern-web-app`; the Rust gateway receives
checked snapshot copies through a bounded reply port. Browser application logic
is compiled Fern WebAssembly. This application evidence does not establish
generalized native actor fairness, typed supervisor trees or multicore scaling.
