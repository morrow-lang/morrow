# Typed local processes

The native `Process` API adds isolated actor failures and independent monitors.
It shares the existing typed `Pid(M)` mailboxes, scheduler owners and copied
messages. Existing `spawn` and `supervise` keep their established behavior.
This is the first stage of the [local OTP plan](superpowers/specs/2026-09-19-typed-otp-process-model.md);
links, exit signalling and the new supervisor policies are still pending.
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
- `Process.Exit(identity, reason)` is reserved for the following links stage;
  this stage does not produce Exit events.

An ordinary receive skips lifecycle cells without consuming them. Sending a
user value that happens to look like an event still produces `Message(value)`;
it cannot forge a system notification. A worker's earlier messages precede its
Down, including when it crosses scheduler owners. Receive guards may compare
opaque identities and references; process operations are forbidden in guards
and defer bodies.

The reason schema is `Normal`, `Shutdown`, `ShutdownDetail(String)`, `Fault(Int)`,
`Failure(String)`, `Kill`, `Killed` and `NoProcess`, under the `Process` namespace.
This stage generates Normal for return, Fault for checked failure and NoProcess
for monitoring an already dead local identity. The other reasons support the
planned signalling API; they are not evidence that those operations exist yet.

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
Each actor can hold 256 monitor completion reservations, with 4,096 across one
invocation. Each reservation charges 512 logical bytes within the existing
64 MiB budget; separate conservative physical accounting covers metadata.
Reservations remain charged through pending and queued notifications, then
release on consumption, cancellation or owner retirement. These slots are
separate from the ordinary 4,096-message mailbox limit, so a full user mailbox
cannot discard an admitted Down.

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
