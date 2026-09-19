# Typed local processes

The native `Process` API adds isolated actor failures, independent monitors,
links and typed exit signals.
It shares the existing typed `Pid(M)` mailboxes, scheduler owners and copied
messages. Existing `spawn` and `supervise` keep their established behavior.
These are the first two stages of the [local OTP plan](superpowers/specs/2026-09-19-typed-otp-process-model.md);
the new supervisor strategies, startup protocol and management policies remain pending.
Stage acceptance is recorded in [the roadmap](../ROADMAP.md).

## Creating and observing a process

| Operation | Result and context |
| --- | --- |
| `Process.spawn(entry)` | `Result(Pid(M), Process.Error)`; root or actor |
| `Process.spawn_monitor(entry)` | `Result((Pid(M), MonitorRef), Process.Error)`; actor only |
| `Process.self()` | Current `Pid(M)`; actor only |
| `Process.id(pid)` | Opaque `ProcessId`; root or actor |
| `Process.monitor(identity)` | `Result(MonitorRef, Process.Error)`; actor only |
| `Process.demonitor(reference, options)` | `Result(Bool, Process.Error)`; owning actor only |

An entry is a zero-argument Unit function with mailbox type `M`, as for ordinary
actors. Atomic spawn-monitor installs the relationship before the child can
execute. Expected admission errors are Results and must be handled. An isolated
child's checked fault retires that child and sends its monitors a `Down`; an
unrelated child can continue. Calling ordinary `spawn` inside an isolated actor
still selects ordinary legacy failure behavior. Root faults, invalid native ABI
use and invocation infrastructure failures remain invocation-wide failures.

`ProcessId` erases mailbox type for observation, without exposing an untyped
send or a conversion back to a typed PID. `MonitorRef` identifies one independent
registration: monitoring the same process twice gives two references and two
notifications. Both opaque values support equality and copying through typed
messages; copies retain the original invocation and generation identity.

See the executable native fixtures for complete examples:
[monitoring a failing worker](../crates/morrow/tests/process_model/monitors.mr)
and [cancelling monitors and receiving events](../crates/morrow/tests/process_model/demonitor.mr).
These are library fixtures with a typed String reply port; their
[native harness](../crates/morrow/tests/process_model_native.rs) supplies the host.

## Receiving events

Ordinary `receive` examines user messages. `receive_event` examines the same
ordered mailbox through `Process.Event(M)`:

- `Process.Message(value)` wraps an ordinary message of type `M`.
- `Process.Down(reference, identity, reason)` reports one monitor completion.
- `Process.Exit(identity, reason)` reports a trapped exit signal.

An ordinary receive skips lifecycle cells without consuming them. Sending a
user value that happens to look like an event still produces `Message(value)`;
it cannot forge a system notification. A worker's earlier messages precede its
Down, including when it crosses scheduler owners. Receive guards may compare
opaque identities and references; process operations are forbidden in guards
and defer bodies.

The reason schema is `Normal`, `Shutdown`, `ShutdownDetail(String)`, `Fault(Int)`,
`Failure(String)`, `Kill`, `Killed` and `NoProcess`, under the `Process` namespace.
Normal return produces Normal, checked failure produces Fault, and observing an
already dead local identity produces NoProcess. Explicit exits and signals can
carry the other typed reasons. Fault payloads retain the full signed 64-bit value.

## Links and exit signals

| Operation | Result and context |
| --- | --- |
| `Process.link(identity)` | `Result((), Process.Error)`; actor only |
| `Process.unlink(identity)` | `Result((), Process.Error)`; actor only |
| `Process.spawn_link(entry)` | `Result(Pid(M), Process.Error)`; actor only |
| `Process.trap_exit(enabled)` | Previous Bool setting; actor only |
| `Process.exit(reason)` | Terminates the current actor; actor only |
| `Process.signal_exit(identity, reason)` | `Result((), Process.Error)`; actor only |

A link is symmetric and has one active relationship per pair. Repeated linking
is idempotent; linking oneself is an inert success. Spawn-link installs both
sides before the child can run and rolls back atomically if admission fails.
Linking a dead local identity creates a NoProcess exit effect. Signalling an
already dead valid local identity succeeds without reviving it.

Actors start with exit trapping disabled. An untrapped Normal signal is ignored;
other untrapped reasons terminate the recipient. With trapping enabled, a linked
or ordinary direct signal becomes an Exit event. An admitted **direct Kill**
terminates with Killed even when trapping is enabled. A locally chosen or linked
Kill is an ordinary reason: it can be trapped and does not force cleanup
suppression. This distinction also applies to self-signalling.

Unlink invalidates still-pending effects from earlier link registrations. It
preserves an Exit already queued in the mailbox and does not cancel independent
direct signals. A new link has a new registration, so cancelling an older one
cannot remove the replacement. A retiring actor publishes its linked exits
before its monitor completions, including when publication spans multiple
bounded scheduler turns.

`Process.exit` evaluates its reason before committing termination. It does not
execute later operands, statements or return continuations. Earlier Result
handling obligations still apply; unreachable handling code cannot satisfy them.
Ordinary termination runs registered cleanup once. The first committed terminal
reason remains authoritative. A checked callback or cleanup fault is recorded
before later signals are processed, so those signals cannot replace its Fault
reason. Checked failures still follow isolated or legacy supervision policy;
an explicit `Process.exit(Process.Fault(code))` remains an explicit exit. A later
admitted direct Kill can suppress cleanup callbacks that have not begun, while
preserving the first reason. A callback or defer
already executing must return before retirement proceeds; native calls are not
interrupted. New user messages to an exiting actor are rejected. Monitors and
links registered before final retirement observe its committed cause.

The [link fixture](../crates/morrow/tests/process_links/links.mr) and
[terminal evaluation fixture](../crates/morrow/tests/process_links/terminal.mr)
provide executable examples; the
[native matrix](../crates/morrow/tests/process_links_native.rs) verifies exact
traces at one, two and four schedulers with stealing off and on.

## Cancellation, lifetime and bounds

`Process.DemonitorOptions(flush, info)` controls cancellation. Without `info`, a
valid owner cancellation returns true. With `info`, true means the registration
was cancelled before its Down was queued; an inactive or consumed monitor
returns false. `flush` also removes an already queued Down for that reference.
Combining `flush` and `info` does not change an inactive result to true. An
already queued Down stays reserved and receivable when cancellation omits flush.
Monitoring oneself creates a fresh inert reference; its info result is false.

Errors distinguish `ResourceLimit`, `ForeignInvocation`, `WrongMonitorOwner`,
`UnsupportedTarget` and `InvalidOptions`. Host ports are not monitor targets.
Each actor can hold 256 directed lifecycle reservations, with 4,096 across one
invocation. Monitors, link directions and direct exit effects share these limits;
a link reserves capacity at both recipients. A future monitor or link completion
reserves 4,609 logical bytes: 512 for the control effect and 4,097 for a possible
NUL-terminated reason string. A direct signal with a known reason reserves 512
plus its actual text size. Unused future-text credit is released when a reason
is materialized. Immutable reason text retained during transport has its own
bounded charge. All logical charges share the existing 64 MiB invocation budget;
physical metadata accounting is separate.

ShutdownDetail and Failure text is limited to 4,096 UTF-8 bytes. Invalid or
oversized direct-signal options return InvalidOptions; unavailable admission
returns ResourceLimit, including for direct Kill. An oversized or unadmitted
local exit becomes checked Fault(9). Malformed native ABI values remain
infrastructure failures. There is no reserved priority lane for Kill.

Reservations remain charged through pending and queued notifications, then
release on consumption, cancellation or owner retirement. These slots are
separate from the ordinary 4,096-message mailbox limit, so a full user mailbox
cannot discard an admitted Down or Exit. Ordinary messages keep their historical
32-byte logical header charge independently of lifecycle metadata size.

A live isolated process may wait indefinitely after main returns. Legacy-only
programs retain their existing deadlock behavior. Native embedders can obtain an
owned cancellation token and request cancellation from another thread; they must
not share a borrowed Exec or operate another scheduler's heap. Cancellation wakes
parked drivers and stops the invocation at safe points. Synchronous native calls
remain cooperative and cannot be forcibly interrupted.

These APIs are native-only; the REPL rejects their use. Distribution, arbitrary
Erlang reason terms, hot code loading and the wider OTP libraries remain outside
this local process effort. [Pinned OTP reference cases](process-model/README.md)
document the external semantic comparisons separately from Morrow's tests and
[actor performance measurements](superpowers/specs/2026-09-19-actor-performance-parity.md).
