# Interactive actors and source simulation

Fern's Rust interpreter runs the same checked actor continuations used by native
compilation. You can define a worker, keep its typed Pid between REPL entries,
and send messages without rebuilding a native executable:

```fern
fn worker():
    receive:
        message -> println(message)

let pid: Pid(String) = spawn(worker)

match send(pid, "hello 🌿"):
    Ok(()) -> ()
    Err(_) -> println("worker has stopped")
```

The actor waits after the spawn entry. The next entry delivers its message,
prints it and completes the worker. Further sends to that Pid return an error.
An old Pid never becomes the identity of a replacement actor.

## Execution model

Each session owns its scheduler, mailboxes and virtual clock. Runnable actors
use a FIFO queue. Selective receive examines messages in arrival order and
removes only the selected message. Immutable values and lexical closures retain
their original checked program; later definitions cannot reinterpret old
function identities. Sharing immutable Rust storage does not permit actors to
mutate one another's values.

The virtual clock advances one millisecond per actor turn. When no actor is
runnable, it jumps to the earliest timeout; no wall-clock sleep occurs. Due
timers continue to make progress while other actors remain runnable. This is a
repeatable scheduling model, not a measurement of physical CPU execution time.
Actors waiting without deadlines remain dormant for future REPL entries.
Native executable deadlock handling still applies to native invocations.

Receiving helpers can return ordinary values, including `Result` payloads.
`with` and `?` preserve short-circuiting, destructuring and error accountability
across suspension. Function values and higher-order List/Option/Result operations
use cooperative continuation calls within their prepared program. A closure
retained from an earlier REPL program keeps its original code and uses the
interpreter's bounded synchronous fallback when passed to a newer actor call;
it never adopts a new function solely because their numeric IDs match. Callback
expressions still evaluate eagerly in source order;
only selected payloads invoke them.

`supervise(entry, max_restarts)` retains an initializer with a budget of zero to
32 restarts. Restarted workers get new Pids. `supervised_current(original)`
returns the current live replacement, while sending to the original dead Pid
continues to fail. Restarts join the ready queue behind existing runnable work.
`defer` belongs to a logical function scope and survives receive suspension.
Callbacks run in reverse registration order on scope return, actor fault or
cancellation. Cleanup faults do not skip remaining callbacks; the original
failure wins. Scopes and pending callbacks share a 4,096-item actor limit and
remain covered by the collector-independent Rust value graph budget.

`:actors` inspects live actors, queued messages, virtual time and pending cleanup.
`:stop` cancels actors while preserving ordinary REPL bindings. `:quit`, EOF and
`:reset` also cancel pending actors and run cleanup. Rust embedders call
`Session::stop_actors()` explicitly before discarding a session when user cleanup
must execute; dropping Rust values alone releases memory without running Fern
code.

The source checker and continuation preflight run before entry effects. A
rejected entry cannot perform file I/O or send messages. If a checked entry
begins executing and then faults, its new bindings are not installed, but
already executed actor effects remain: a message sent before a fault can be
processed by a later entry. Output from a failed entry follows existing REPL
error behavior. Unsupervised actor faults stop that evaluation and retire the
faulting actor; other queued actors remain available to later entries.

## Bounds and replay

A session permits 256 live actors and 4,096 queued messages in total, across all
entries. Actor value graphs and retained code have a 16 MiB storage budget;
ordinary REPL retained bindings have their existing independent budget. Actor
identities increase monotonically and never wrap. Per-entry execution has the
interpreter's 100,000-step budget and a separate bounded cleanup budget. These
limits apply to actors collectively, including restarts and selective scans.

Rust embedders can inspect `Session::actor_report()` without executing work.
`fern_compiler::repl::simulate_actors(entries)` runs a reproducible transcript
with filesystem, network, real-clock and foreign effects prohibited. Pure
language operations, captured output, typed JSON operations and virtual actors
remain available. Inputs are bounded to 4,096 entries / 8 MiB; captured
transcripts are bounded to 8 MiB. Failed entry outcomes remain in the report. A `":stop"` transcript entry records
explicit cancellation and its cleanup output.

`fern_sim::language::run(entries)` exposes the same source simulator through
FernSim. `language::replay(entries, expected)` compares every outcome and final
scheduler counter, rejecting changed or corrupted reports. The existing
FernSim native-actor and web/protocol fault campaigns remain separate tools.

Acceptance includes an independent Rust FIFO/round-robin model for 240
full-width messages over three seeds, exact replay, selective mailbox retention,
virtual deadline fairness, supervised cleanup and sibling progress, suspended logical scopes, cleanup
fault precedence, explicit/terminal cancellation, quota
recovery without Pid reuse, cross-entry mailbox caps, Result obligations and
binding rollback after executed effects. The FernSim integration verifies its
own expected Unicode/full-width transcript and rejects a corrupted report.

Result sequencing additionally has a three-seed, 54-case independent Rust model
for first/second-step errors, shared handlers, tuple and Unicode payloads, and
logical cleanup order. Native tests force collection at suspension boundaries
and check sibling progress. Option/Result combinator tests distinguish eager
callback factories from skipped callbacks. Forged `with` metadata is rejected,
including refutable bindings and expansion beyond the continuation work budget.
