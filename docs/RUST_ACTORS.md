# Rust native actor execution (Decision105A)

Status: accepted bounded implementation. Typed native execution is 105A; generalized suspension, typed supervision, and deterministic FernSim execution remain later stages.

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

`spawn(entry)` takes a zero-argument function returning Unit and returns invariant opaque `Pid(M)`. Its captures are evaluated once; its body is queued, never run inline. `send(pid, message)` returns `Result((), Int)`: Ok means enqueue only; Err 3 means dead/foreign identity, Err 4 means mailbox/session quota or an unaccounted message graph. Send borrows its message and gives no Result-handling or transfer credit. Result-bearing messages, callable messages, and unaccounted native handles are rejected by checking. Result-bearing ordinary closure captures retain the existing prohibition. Compiler-created continuation frames may retain already-owned local Result duties; suspension is neither a completed exit nor handling credit, and every completed actor path must still satisfy the ordinary Result proof.

Mailbox schemes are inferred from the owned receive patterns, with no arbitrary scalar default; source body constraints do not supply additional mailbox inference in this checkpoint. An indented `receive` selectively considers messages in enqueue order and arms in source order. Unmatched messages remain in order. Guards are bounded pure, nonallocating, nonfailing scalar expressions without calls. Duplicate or unreachable arms are rejected, but receive need not be exhaustive. An optional final `_ after duration -> body` evaluates duration once; it must be an Int in 0..600000 milliseconds. Registration first tries existing queued messages, even with zero duration. Subsequent polls consider only messages committed strictly before the absolute monotonic deadline. Timely messages do not lose because another actor delayed polling. At exact millisecond equality the deadline wins. Timeout fires only after no eligible message matches. Timer wake ordering is deadline, then stable actor identity. Timeout expressions and capture graphs are not reevaluated on wake.

Receiving functions return Unit and may suspend in tail position, block statements/initializers, If/Match branches, and explicit returns. Direct receiving calls in tail position update a continuation frame. Receiving-call Result arguments currently retain their caller duties; an otherwise valid callee-based discharge may be conservatively rejected until receiving-call summaries are proved. Non-tail receiving calls, receive inside For/With or strict operands, arbitrary indirect receiving calls, and receiving functions owning defer are diagnosed as unsupported. Ordinary pure spawned functions retain ordinary function-exit defer behavior. Calls into ordinary helpers retain their normal cleanup behavior. An actor suspension never runs defer.

The REPL rejects 105A actor programs before effects or retained definitions change. Mailbox actor APIs and their supervision policies remain separate; complete FernSim parity is not claimed.

## Execution ABI and provenance

Ordinary generated ABI remains `(environment, fault, source arguments)` with an exactly 8-byte fault slot. Context-requiring direct entries use `(environment, fault, execution context, source arguments)`. Context is never captured into a source closure, stored in a global current-actor variable, or read from beyond the fault slot. First-class context-requiring ordinary helpers are rejected except an immediate spawn entry; pure first-class callbacks keep the ordinary ABI.

The native step callback is `int64_t(exec*, frame*)`; selectors are `void*(exec*, frame*, int64_t payload)`. Cranelift transports both native status and payload as 64-bit words. Immutable function descriptors bind exact code identity, capture count/types, callback kind, and mailbox type. Type descriptors have four 64-bit words: kind, count, child pointers, and sum arities. Public IR is validated before private CPS conversion; caller-created IR cannot construct the opaque private lowered operations. Original and inactive signatures, actor metadata, closure identities, and capture arity/types are checked before cloning. Unknown identities have no fallback.

A selected frame is allocated only after the complete pattern and guard succeed. Registration validates both selector and timeout identities before charging or publishing roots. Its new selector/timeout captures replace the spent entry root. Selection or timeout installs the successor before retiring old receive roots. Completion/cancellation clears frames, selectors, timeout state, messages, and queue links; dead identity metadata remains until the invocation is collected. The runtime uses the existing GC heap for retained source values. Small bounded temporary graph indices use Rust-owned collections and are reclaimed on all paths.

## Ownership, quotas, and failures

One invocation owns a FIFO cooperative scheduler, immutable PID identities, and mailboxes. Main executes first. Successful main drains actors; main faults or returned Err stop pending actors without running their bodies. The first actor fault stops the session after any active ordinary helper cleanup. A waiting session without a runnable actor or pending timer reports deadlock. Blocking host calls and nonyielding source computation can delay scheduling;105A does not promise preemption or arbitrary wall-clock bounds.

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

Limits are 1024 live actors,65536 lifetime identities,4096 messages per actor,65536 queued messages globally, and 64 MiB aggregate logical retained ownership. Descriptor tables and value graph indices each contain at most 4096 entries, with 128 payload-depth limit. Descriptor registration shares 1,048,576 work units across identity and metadata inspection; each enqueue/frame graph attempt shares the same finite allowance across descriptor work, identity lookup, graph traversal, and 64-byte String scan units. Immutable DAGs share within one owner graph; separate enqueues are charged separately. Unknown native object graphs are not treated as scalar pointers. PID graphs must belong to the same session and exact mailbox identity.

Enqueue validates the graph and reserves bytes before allocating. Its monotonic timestamp is read at commit after potentially expensive validation/allocation; a clock failure rolls back the reservation and leaves the mailbox unchanged. Failed sends never remove or reorder messages. Receive validates duration, selector, timeout mailbox, and clock before publishing roots. Checked clocks reject invalid/overflowing time representations. GC storage is distinct from this logical retained quota; retiring roots makes values collectible but does not claim immediate memory reclamation.

Fault 8: `actor timeout must be between 0 and 600000 milliseconds`.
Fault 9: `actor resource limit exceeded`.
Fault 10: `actor deadlock: no runnable actor or pending timeout`.
Fault 11: `invalid actor execution descriptor`.
Fault 12: `actor monotonic clock failure`.

Existing fault codes 1..7 and their first-failure behavior remain unchanged. Actor failures use the ordinary `fern: runtime error: ...` diagnostic and exit 1. No source Result type is silently rewritten to encode execution faults.

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
