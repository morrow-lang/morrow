# Deterministic native actor simulation

Morrow can drive its real native managed actor runtime with an invocation-local
virtual clock. This is an opt-in testing capability, not a replacement scheduler
or a claim of Erlang/OTP production maturity. It exercises the same descriptor
validation, graph copying, actor heaps, FIFO scheduling, selective receive,
supervision, identity reuse and host reply ports used by compiled native Morrow.
The scenario callbacks themselves are small Rust implementations of the native
callback ABI; the separate [application simulator](DETERMINISTIC_SIMULATION.md)
uses the compiled Morrow room actor through `NativeDomain`.

## Run and replay

```sh
cargo xtask simulate actor-run --seed 42 --steps 10000 --json
cargo test -p morrow-runtime --no-default-features --features simulation managed::simulation
```

The `morrow-sim actor-run` command also works directly. A run records its scenario
version, seed, step count, virtual milliseconds, scheduler turns, delivery/timeout
counts, restarts, churn, trace hash and final heap accounting. Version 1 fixes
SplitMix64 event generation and a little-endian FNV-1a trace hash. Neither is used
for security. Replaying requires the same scenario version and runtime source
revision; preserve the binary or Git revision alongside a failing report.

The seed selects external events and host poll budgets. The production scheduler
remains FIFO. Changing a seed varies events; it does not enumerate every possible
schedule. Steps must be in `1..=1_000_000`, and all loops, reply buffers and work
budgets are bounded. Each round advances at most ten virtual minutes. This bound
keeps scenario time far below `u64` overflow even at the maximum step count.

Rust callers use the safe entry point:

```rust,ignore
use morrow_runtime::managed::simulation::{run, Config};
let report = run(Config { seed: 42, steps: 10_000 })?;
assert_eq!(report.final_heap_objects, 0);
```

The runner creates and joins a dedicated runtime thread. Managed pointers and
roots never cross that boundary. Success, a failed assertion, or a Rust unwind
releases the runner's invocation and heap storage without collecting the caller's
heap. Failures contain the full configuration, failing step and oracle message.
A runtime process abort, operating-system kill or undefined behavior cannot be
converted into a normal simulation failure.

## Independent checks

Each round creates a real selective-receive actor and a sibling that yields
through real continuation publication. The driver checks expected replies rather
than merely comparing two runs of the same implementation:

- A matching message timestamped before the deadline wins even if it is processed
  at the deadline. A message timestamped exactly at the deadline loses.
- An unmatched earlier message remains selective; a later matching message does
  not override the expired receive. Termination reclaims discarded messages.
- Deadlines equal independently computed virtual times. Empty ports stay empty
  before any timer can fire. Ten-minute waits require no wall-clock sleep.
- A yielding sibling completes within a fixed number of bounded polls. Expected
  FIFO reply order accounts for whether that sibling needs another continuation.
- A faulting supervised child emits from its original initializer on every
  restart, despite mutating its own copy before failure. Exhausting the configured
  lifetime restart budget leaves the unrelated sibling and invocation healthy.
- Completed actors release their logical quota. Between rounds only the host port
  remains live, with no messages or active deadlines. Forced precise collection
  before timer sends tests explicit host/port/timer PID roots, and sender/selector collections test callback
  temporaries. Periodic collection also exercises reclamation between rounds. Final close and precise collection leave zero
  physical managed bytes and objects; the invocation's own control quota persists
  until close and is checked separately from actor quota.

A fixed corpus includes a run exceeding the old 65,536 lifetime identity limit.
A failure sentry executes the same real runtime with a deliberately reversed
expected timer result and requires an error containing the replay configuration.
Additional boundary tests reject backward/overflowing clock changes atomically,
reject a deadline that would overflow, and inject one clock-read failure into a
send while checking that no message/quota mutation or other-session fault occurs.
A constructor regression also forces precise collection before the identity-table
allocation in each of 32 fresh invocations. The new Session must remain explicitly
rooted before the host can publish its persistent Exec root; each invocation then
creates a virtual-clock port, polls, closes and reclaims all managed objects.
The decode-to-actor regression covers empty and populated Lists and Maps, exact
UTF-8 keys and full-width integer payloads after sender collection. A separate
nested codec fixture forces collection during record/list/Option/Map/tuple/sum/
union construction and checks that no partial object disappears before publication.
Decoded empty lists reserve actual native storage, and Map actor copies use the
compiler's untagged key/value entry layout. These scoped roots cover typed JSON
decoding; they are not an assertion that all native JSON helpers or all runtime
allocation sites have completed the precise-root audit.
These supplement the existing compiler/native actor oracles; they do not replace
compiled-language tests.

The `callbacks` report field counts scheduler turns, including a ready waiting
actor being polled. It is not a count of machine instructions, all selector calls,
wall time, or BEAM reductions. Timer promotion can invoke selectors outside that
counter. Resource counts describe Morrow managed storage, not total process RSS.

## Host clock API

Enable Cargo feature `morrow-runtime/simulation` to expose
`morrow_runtime::managed::simulation`. Builds with this feature disabled contain
neither these controls nor the per-session virtual clock fields. Cargo workspace feature
unification can enable it for a shared runtime build, because `morrow-sim` requests
it; package-specific production builds do not request it by default. Runtime unit tests also
compile the seam, while retaining their older private test-clock oracles.

An embedding host can use these unsafe APIs on a live, rooted `Exec`:

```rust,ignore
enable_clock(exec, start_ms)?;
advance_clock(exec, later_ms)?;
fail_next_clock(exec)?;
let state = snapshot(exec)?;
```

Enable before publishing any actor identity, including a host port. Time is
monotonic per invocation; `u64::MAX` is reserved for the no-deadline sentinel and
cannot be selected. Rejected changes do not mutate time, consume an injected
failure or poison the fault cell. `fail_next_clock` affects exactly the next
clock read in that invocation. A snapshot never reads the wall clock or executes
a callback. `ClockError` distinguishes invalid context, already-enabled/started,
not-enabled, backward and overflow cases.

All calls must run on the invocation's owner thread outside actor callbacks. The
host must keep the `Exec`, descriptor table, fault cell and referenced native
values rooted and valid until close. The APIs are unsafe because these raw-pointer
lifetime requirements cannot be checked by Rust's type system. They do not make
an `Exec` transferable between threads. Drive virtual sessions with
`morrow_managed_poll` and explicit time advancement. `morrow_managed_run` is the
ordinary blocking driver and must not be used for virtual waits: it does not
advance a virtual clock automatically.

## What the OTP study informed

The study used the official Erlang/OTP repository at tag `OTP-29.0.6`, resolved to
commit `e07fd07837e5aa845657f5fa340637121e451d47`. Only six source/document files
were fetched (586,454 bytes), rather than cloning the repository. No OTP
implementation was copied into Morrow.

1. **Scheduling has to preserve resumable state.** In the pinned
   [BEAM emulator](https://github.com/erlang/otp/blob/e07fd07837e5aa845657f5fa340637121e451d47/erts/emulator/beam/emu/beam_emu.c#L358),
   the emulator computes consumed reductions, calls the scheduler and restores
   saved registers. The
   [process record](https://github.com/erlang/otp/blob/e07fd07837e5aa845657f5fa340637121e451d47/erts/emulator/beam/erl_process.h#L1062)
   carries the remaining reduction count. The lesson for Morrow is to test actual
   resumable boundaries and preserved state; a host-side counter cannot preempt
   an arbitrary synchronous helper.
2. **Restart policy has explicit temporal semantics.** The pinned
   [supervisor implementation](https://github.com/erlang/otp/blob/e07fd07837e5aa845657f5fa340637121e451d47/lib/stdlib/src/supervisor.erl#L2260)
   tracks monotonic restart times and filters a rolling window. OTP's documented
   [intensity and period](https://www.erlang.org/docs/27/apps/stdlib/supervisor.html)
   therefore differ from Morrow's current `0..=32` lifetime restart budget. This
   work preserves Morrow's policy and tests exhaustion; it does not silently claim
   OTP-compatible supervision.
3. **Test observable survival and restart behavior.** The pinned
   [supervisor suite](https://github.com/erlang/otp/blob/e07fd07837e5aa845657f5fa340637121e451d47/lib/stdlib/test/supervisor_SUITE.erl#L3197)
   checks child exits and whether the supervisor remains alive across termination
   modes. Morrow's scenarios similarly check emitted values and unrelated sibling
   survival, without adopting OTP's test suite or its wall-clock waits.
4. **Replay needs an explicit algorithm and seed.** The official
   [rand documentation](https://www.erlang.org/doc/apps/stdlib/rand.html)
   distinguishes explicit PRNG state and algorithm selection for reproducibility.
   [Common Test's shuffled groups](https://www.erlang.org/docs/27/apps/common_test/write_test_chapter.html#shuffled-test-case-order)
   record a seed so an execution order can be replayed. Morrow records an explicit
   integer-only algorithm version and seed; a trace hash adds a compact regression
   signal but cannot substitute for behavioral assertions.

## Limits and next work

This seam provides deterministic evidence for the implemented native callback
runtime. It does not establish general preemption, multicore actor migration,
network partitions, distributed Erlang compatibility, hot code upgrades, durable
runtime heaps, every supervision strategy or production reliability under years
of load. Native fairness currently covers generated actor continuations and the
eligible recursive `Unit` tail-helper subset described in
[ACTOR_RUNTIME.md](ACTOR_RUNTIME.md); ordinary synchronous helpers can still hold
the invocation thread. The scenario uses short bounded callbacks and must not be
read as proof of fairness for those remaining helper forms.

Full reduction scheduling still needs general resumable computation and precise
live state at every suspension boundary. Rolling restart intensity needs an
explicit language policy and independent window-boundary tests. Distributed
runtime testing needs real transport/reconnection/fencing boundaries. Those are
separate implementation milestones, even though this clock seam makes their
future deterministic tests easier to build.
