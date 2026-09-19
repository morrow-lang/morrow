# Local process reference cases

These are independent OTP reference fixtures, not Morrow implementation code.
`monitor_reference.exs` runs on Elixir 1.20.4 / Erlang OTP 29.0.6 (ERTS 17.0.6 JIT).
Its 13 lines match `monitor_reference.expected` exactly at `+S 1:1`, `+S 2:2`
and `+S 4:4`; each invocation exits zero with empty stderr. This verification
was performed on 2026-09-19. Five-second receive bounds are failure watchdogs,
not synchronization. Process messages and monitor notifications establish all
ordering; independent monitors are selected by their own references.

Run using the pinned comparison toolchain:

```sh
ERL_FLAGS='+S 1:1' elixir docs/process-model/monitor_reference.exs
```

Privately extracted Homebrew bottles additionally need `ERL_ROOTDIR` set to their
`lib/erlang` directory and that Erlang installation on PATH. No global toolchain
installation is required.

The reference covers two distinct monitors, exact full-width payload ordering,
selective receive leaving a lifecycle event queued, dead-target monitoring,
cancellation before target completion, consumed/repeated demonitor options, and
self-monitor behavior. The `{fault, 1}` reason is deliberately explicit; it
compares the selected local lifecycle contract without depending on a particular
Erlang exception stack or Morrow diagnostic representation.

An initial hand-written expectation for `consumed_flush_info` was true. The
pinned runtime produced false, and its `demonitor_2` implementation confirmed the
result. The expected trace and Morrow contract were corrected from that external
evidence. `info + flush` does not mean “no matching cell was found”; an inactive
monitor returns false even after its Down was consumed. A self-monitor gets a
fresh reference but no active monitor. See the
[pinned BIF source](https://raw.githubusercontent.com/erlang/otp/OTP-29.0.6/erts/emulator/beam/bif.c).

`link_reference.exs` has 15 fixed observations, also checked at one, two and four
schedulers on that pinned runtime with empty stderr. It covers idempotent and
atomic links, normal-signal handling, direct versus linked Kill, unlink before
death and after an Exit is queued, explicit signals independent of unlink,
dead targets, self-links and self-normal signalling. A death monitor is the
barrier before absence assertions. Direct Kill becomes Killed even when
trapping; a linked exit with reason Kill follows the ordinary trap rules and
retains Kill. These are distinct cases in the
[official process contract](https://www.erlang.org/doc/system/ref_man_processes.html).
The fixture uses OTP 29's `exit_signal/2` to match the proposed separate signalling
API; it does not use the older `exit(self(), normal)` special case.

`supervisor_reference.exs` records 16 observations at the same three scheduler
counts, again with exact stdout and empty stderr. An acknowledged worker and
synchronous event log establish ordered startup, reverse shutdown,
one-for-one/one-for-all/rest-for-one restart traces and fresh identities. Manual
termination retains a permanent child's specification; deletion and temporary
termination remove it. Expected child-exit diagnostic reports are disabled in
the reference process, while watchdog exceptions and trace assertions still
fail the run. This fixture does not cover rolling intensity boundaries, failed
initialization, significant-child shutdown or deadline escalation; those need
additional oracles before supervisor acceptance.

`supervisor_policies_reference.exs` adds 34 observations, verified on the same
pinned toolchain at one/two/four schedulers with exact output, zero exit status
and empty stderr. It covers permanent/transient/temporary reasons (including
ShutdownDetail), initial rollback, ignored children, failed restart attempts,
failed group initializer retry points, manual restart at intensity zero,
significant-child automatic shutdown, Graceful(0), Infinity and unlink.
A nested shutdown handshake also proves that a killed branch can be reported
dead while its trapping leaf is still alive; explicit release then retires that
leaf with Shutdown. Neither the fixture nor the Morrow plan equates branch
retirement with a hard deadline for every descendant. Exact rolling-window and
deadline boundaries still require injected-clock native tests; this fixture
avoids elapsed-time assertions.

Passing this reference fixture does not verify Morrow. Its compiler/native
fixtures, deterministic races, resource accounting, migration, cancellation and
full repository gate remain separate acceptance requirements. Morrow's typed
reasons, finite quotas, explicit ownership errors and legacy failure policy are
deliberate differences recorded in the process-model design.
