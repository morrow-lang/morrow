# Actor performance parity

Status: proposed. Date: 2026-09-19. Baseline commit:
`11e6ad5b8bb6058dd61e22d25ef94440b5c5a2b2`.

## Goal and claim boundary

Reach **defined-matrix performance parity** with Elixir/BEAM on the existing
actor comparison without changing its workloads, expected outputs, scheduler
counts or primary timing interval. Parity means that Morrow with work
stealing enabled and the default `MORROW_REDUCTIONS=1` reaches at least the BEAM median in every
unchanged throughput cell. A median ratio from `0.80` through `<1.00` is a
competitive band, not parity.

This goal does not mean feature parity with Erlang/OTP. It does not imply equal
supervision, failure propagation, native-work scheduling, selective receive,
distribution or production fault isolation. Those gaps are listed separately so
improving the measured workloads cannot silently broaden the claim.

Overall actor completion has two independent acceptance axes: this strict
performance matrix and the
[typed OTP process model](2026-09-19-typed-otp-process-model.md). Passing this
matrix cannot complete the process-model work, and implementing the process
model cannot substitute for the numeric targets below. The process-model design
owns the semantic scope; this document retains a checklist only to keep benchmark
claims within that boundary.

## Frozen acceptance matrix

The source workloads and independent oracles in
`benchmarks/language-comparison` remain unchanged:

| Workload | Arguments | Primary measured work |
| --- | --- | --- |
| Request/reply | `request-reply 32 500` | 16,000 verified serial replies through one shared server |
| Contention | `contention S 257 2000000` | 257 probes plus the verified hot recurrence |
| Lifecycle | `lifecycle 512 128` | 512 clean actors plus 128 actors with one checked restart |

Each workload runs with one, two and four schedulers. BEAM receives `+S S:S`.
The parity candidate is `MORROW_WORK_STEALING=1` and
`MORROW_REDUCTIONS=1`; pinned Morrow remains a diagnostic control. The
reductions=32 variant remains experimental and cannot satisfy parity unless it
becomes the shipped default before the final measurement.

The primary interval remains `ready` through the verified summary for
request/reply and contention. Lifecycle remains `ready` through observed stream
closure so both runtimes include final retirement and shutdown. Process startup
is retained as a separate wall-time field and cannot substitute for actor-time
parity.

## Numeric acceptance

The first gate reruns the exact current protocol: one warmup, five measured
rounds, rotating runtime order, no outlier removal. It passes only when all of
the following hold:

- Every process passes its independent semantic oracle, emits no stderr and
  stays within the existing output and 180-second bounds.
- For each of the nine workload/scheduler cells, the ratio
  `median(Morrow stealing reductions=1 operations/s) / median(BEAM operations/s)`
  is at least `1.00`. Cells are not averaged; a faster cell cannot hide a slower
  one.
- At two and four schedulers every contention run retains all 256 adjacent probe
  intervals under load. External pipe-observed gaps remain diagnostic only.
- The baseline, compiler, runtime archive, workload, runner, BEAM and source
  hashes are recorded with all raw streams and event timestamps.

After the first gate passes, a confirmation gate runs only the parity candidate
and BEAM for two warmups and nine measured, rotated rounds. Every median ratio
must still be at least `1.00`, and the lower quartile of paired round ratios in
each cell must be at least `0.80`. This second condition is a repeatability guard;
it does not redefine `0.80` as parity.

The final result must report ratios as well as absolute values. The baseline
ratios below show the distance from the target at commit `11e6ad5`:

| Workload | 1 scheduler | 2 schedulers | 4 schedulers |
| --- | ---: | ---: | ---: |
| Request/reply | 0.247 | 0.072 | 0.123 |
| Contention scenario | 0.036 | 0.022 | 0.023 |
| Lifecycle scenario | 0.412 | 0.127 | 0.163 |

The contention rate includes the hot integer recurrence. That makes it a whole
scenario result rather than pure mailbox throughput, but it remains in the
acceptance matrix because removing or weakening it would move the existing
goalposts.

## Measurement work

1. Preserve the current result and profiles before changing code.
2. Profile one workload at a time with the exact release artifact. Treat sampled
   runs as diagnostic evidence; never combine their timings with quiet matrix
   results.
3. Develop bounded runtime or compiler changes with independent red-to-green
   oracles, focused checks and diagnostic comparisons. Require the normal
   repository gate, ThreadSanitizer and exact deterministic replay before
   acceptance measurement.
4. Run a release semantic smoke against all variants.
5. Run the unchanged five-round matrix in a quiet period. Preserve any failure
   or timeout without retrying, trimming or changing the bound.
6. If every strict cell passes, run the nine-round confirmation gate in a second
   quiet period and publish both result directories.

No new benchmark dependency is required. The existing standard-library Rust
runner remains responsible for bounded child supervision, independent formulas,
raw output and provenance.

## Future coverage

The strict goal above is intentionally tied to the already published matrix.
The following additive workloads are needed before making a broader actor-runtime
performance claim. They are unchecked future coverage and are not secretly
considered complete by passing the nine current cells.

- [ ] `sharded-request-reply SERVERS CLIENTS REQUESTS`, with one server and one
  server per scheduler, 32 clients and 5,000 requests, to separate the shared
  server bottleneck from parallel scaling.
- [ ] `payload-request-reply CLIENTS REQUESTS WORDS`, with 0, 16 and 256 flat
  integer words, to measure ordinary message copying without BEAM's special
  reference-counted binary path.
- [ ] `mailbox-burst PRODUCERS MESSAGES WORDS`, initially 16 producers by 512
  messages with 0 and 32 words, to measure ingress, backlog and drain behavior.
- [ ] `fairness HOT_ACTORS PROBES WORK`, with `HOT_ACTORS = 2 * S` and 1,024
  probes, true internal monotonic timestamps, per-hot-actor service counts and a
  guaranteed load-overlap marker.
- [ ] `lifecycle-waves WAVES WORKERS FAULTS RESTARTS`, initially ten waves of
  the existing 512/128/1 case plus a zero-fault control, to measure sustained
  creation, retirement and recovery.
- [ ] A deterministic imbalanced work pool with `2 * S` runnable actors, to
  exercise load redistribution and migration without asserting undocumented
  scheduler placement.

Each future throughput or lifecycle cell uses the same strict `>=1.00` median
ratio before it can be called parity. Fairness needs a non-pinning monotonic time
source first. Its eventual target is complete load overlap, internal request RTT
p99 no greater than `1.25x` BEAM, maximum no greater than `2x` BEAM, and each hot
actor's service count at least `0.80` of the median. Until then the existing
external marker gaps remain useful regression evidence, not latency parity.

## Feature-parity checklist

Performance parity is separate from semantic parity. The checklist is grounded
in the behavior documented by Erlang/OTP rather than similarity of names. The
typed OTP process-model design is authoritative for implementation and acceptance
of these features.

| Area | BEAM behavior | Current Morrow comparison | Claim |
| --- | --- | --- | --- |
| Scheduler count | `+S` controls normal scheduler threads; dirty CPU and I/O schedulers are separate. | `MORROW_SCHEDULERS` matches the normal count for these pure actor workloads. | Numeric count only |
| Preemption | Process scheduling is pre-emptive after a reduction budget is consumed. | Native continuations have a configurable callback budget. | Implemented mechanism, not exact reduction semantics |
| Native work | Long native functions must yield or run on separate dirty schedulers. | Arbitrary FFI conservatively pins its actor; no dirty-scheduler analogue is measured. | Gap |
| Message copying | Ordinary local process messages are copied; literals and reference-counted binaries are exceptions. | Validated typed graphs are copied. Explicit flat payload shapes can be compared. | Comparable subset |
| Links | Links are bidirectional, unique between two processes and propagate exit signals. | The current comparator has no link oracle. | Gap |
| Monitors | Monitors are unidirectional, repeated monitors have independent references, and termination produces `DOWN`. | Elixir uses `spawn_monitor`; Morrow uses bounded lifetime supervision. | Gap |
| Supervisors | Child restart modes, strategies, restart-intensity windows, ordered shutdown and kill timeouts are observable behavior. | The lifecycle workload implements one bounded restart, not an OTP supervisor tree. | Gap |
| Selective receive and priorities | Mailbox scanning, monitor references and process/message priorities affect scheduling and delivery. | Selective receive is implemented; this comparison does not certify BEAM semantics, and priorities/reference semantics remain separate work. | Coverage and feature gaps |
| Distribution | Signals, monitors and failure across nodes have separate ordering and disconnection behavior. | The comparison is single-node and in-process. | Gap |

Primary references:

- [ERTS process scheduling and reductions](https://www.erlang.org/docs/27/apps/erts/erlang.html)
- [`erl` normal and dirty scheduler flags](https://www.erlang.org/doc/apps/erts/erl_cmd.html)
- [Dirty NIF scheduling and duration rules](https://www.erlang.org/doc/apps/erts/erl_nif.html)
- [Erlang links, monitors and signals](https://www.erlang.org/doc/system/ref_man_processes.html)
- [Elixir 1.20 Supervisor restart and shutdown behavior](https://hexdocs.pm/elixir/1.20.4/Supervisor.html)
- [Erlang message copying and SMP guidance](https://www.erlang.org/doc/system/eff_guide_processes.html)

## Evidence and deliverables

The immutable baseline matrix is
[`actors-macos-arm64-20260919`](../../../benchmarks/language-comparison/results/actors-macos-arm64-20260919/).
Its workload SHA-256 is
`9de8abc625272d941a9e42aaa7ad7a25d09fe48e08012d8c78ed66327bcd0556`.
Baseline profiles live in
[`actors-parity-baseline-20260919`](../../../benchmarks/language-comparison/results/actors-parity-baseline-20260919/)
with exact inputs, raw stack samples and file hashes. Profile wall times include
sampling overhead and are not acceptance measurements.

Completion requires:

- a fresh unchanged-matrix result with all nine strict ratios at or above 1.00;
- the separate nine-round confirmation result;
- exact raw streams, event timings and artifact/source manifests;
- a concise before/after bottleneck account tied to preserved profiles; and
- an explicit list of unchecked future workloads and remaining OTP feature gaps.
