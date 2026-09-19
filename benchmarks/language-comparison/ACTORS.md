# Morrow and Elixir/BEAM actor comparison

This suite supplies direct, bounded actor workloads for the scheduler milestone.
The dated results below are one reproducible comparison on the recorded host.
The dated reports elsewhere in this directory remain unchanged historical
evidence.

## Workloads and independent checks

Both implementations expose the same three runtime commands:

```text
request-reply CLIENTS REQUESTS
contention SCHEDULERS PROBES WORK
lifecycle WORKERS FAULTS
```

`request-reply` starts reusable clients and one server. Every client sends the
next request only after receiving its reply. The Rust harness derives the total
reply count and checksum with arithmetic-series formulas; it does not repeat the
actor loop.

`contention` initially places one recurrence actor on the responder's Morrow
scheduler by accounting for documented root-spawn round-robin placement. The
probe actor performs serial request/reply and emits ordered completion markers.
The recurrence checksum uses modular exponentiation in the Rust oracle. A
`hot-done` marker identifies which adjacent probe completions were observed while
the load actor was still active. Work stealing may move either actor after its
initial placement.

`lifecycle` starts ordinary short-lived actors and faulting actors. Each faulting
Morrow actor sends an attempt record, triggers a checked list fault, restarts
once, then exhausts its lifetime budget. Elixir uses a small `spawn_monitor`
restarter with the same one-restart contract. Closed-form sums independently
check completions, attempts, worker values and fault identities.

Every program prints `ready` immediately before workload dispatch. The runner
records process wall time separately from actor work, so startup remains visible.
Request/reply and contention use `ready` through the verified summary. In
`lifecycle`, the summary follows the last attempt publication, while the measured
scenario interval runs from `ready` through observed stdout/stderr closure and
therefore also includes the final fault, retirement and shutdown without the
runner's process-status polling interval. The Elixir driver waits for
the final monitored `DOWN` after printing the summary, matching Morrow's automatic
drain before exit. Lifecycle operations per second is a full-scenario rate, not a
pure actor-retirement latency measurement.

## Measurement protocol

The baseline matrix uses one, two and four online schedulers. Morrow runs both
without stealing and with `MORROW_WORK_STEALING=1`, always with
`MORROW_REDUCTIONS=1`. Elixir receives the matched `+S N:N` setting. The optional
`--include-tuned-32` flag adds a clearly named
`morrow-stealing-r32-experimental` variant; it is not part of the baseline.

Each matrix cell retains one warmup followed by five measured rounds. Runtime
order rotates each round. The runner keeps exact stdout, stderr and event
timestamps, rejects output beyond 1 MiB, rejects any unexpected stderr or
semantic mismatch, and limits each child to 180 seconds. Summaries use medians
without outlier deletion and also retain the maximum per-run p99 completion gap.

Probe timing is an external observation. It includes `println`, pipe delivery
and reader scheduling, and several lines can be observed close together. The
reported values are therefore **observed inter-completion gaps**, not internal
mailbox latency. First-probe delay is separate. Quantiles include only intervals
whose ending marker was observed no later than `hot-done`; the raw count makes a
run with little or no load overlap visible instead of treating it as a CPU-load
tail result. Morrow currently exposes no source-level monotonic timestamp that
would improve this without changing actor placement through an arbitrary FFI
call.

This is not an OTP parity claim. Morrow's typed actors copy validated message
graphs, retain pinned ownership unless stealing is enabled, and use a bounded
lifetime restart count. The Elixir lifecycle comparator is not an OTP Supervisor
tree, and the suite does not compare links, monitor-reference semantics,
restart intensity windows, distribution, arbitrary external blocking calls or
production fault isolation. Equal message-level results do not imply equal heap,
collector or scheduler work.

## Final results: 2026-09-19

The [complete result directory](results/actors-macos-arm64-20260919/) contains
216 process observations: one warmup and five measured rounds for every cell.
All processes passed the independent output oracle. The table reports median
measured operations per second for scheduler counts 1 / 2 / 4; raw values and
all timing fields remain in
[`measurements.csv`](results/actors-macos-arm64-20260919/measurements.csv).

| Workload | Implementation | 1 scheduler | 2 schedulers | 4 schedulers |
| --- | --- | ---: | ---: | ---: |
| Request/reply | Morrow pinned, reductions 1 | 284,430 | 215,320 | 208,123 |
| Request/reply | Morrow stealing, reductions 1 | 284,480 | 190,320 | 192,992 |
| Request/reply | Morrow stealing, reductions 32 experimental | 280,321 | 210,634 | 217,612 |
| Request/reply | Elixir/BEAM | 1,150,017 | 2,648,404 | 1,566,426 |
| Contention scenario | Morrow pinned, reductions 1 | 583 | 550 | 547 |
| Contention scenario | Morrow stealing, reductions 1 | 585 | 522 | 547 |
| Contention scenario | Morrow stealing, reductions 32 experimental | 583 | 587 | 582 |
| Contention scenario | Elixir/BEAM | 16,154 | 23,863 | 23,551 |
| Lifecycle scenario | Morrow pinned, reductions 1 | 82,830 | 100,846 | 99,072 |
| Lifecycle scenario | Morrow stealing, reductions 1 | 83,019 | 28,080 | 29,214 |
| Lifecycle scenario | Morrow stealing, reductions 32 experimental | 83,157 | 29,122 | 29,734 |
| Lifecycle scenario | Elixir/BEAM | 201,436 | 221,968 | 179,287 |

BEAM completed the in-process request/reply work 4.0x, 12.3x and 7.5x faster
than pinned Morrow at one, two and four schedulers. Morrow's process wall time
was nevertheless lower for request/reply and lifecycle because BEAM startup took
roughly 127--139 ms outside the measured actor interval. Consumers should choose
the interval that matches their deployment model instead of mixing startup and
actor execution. Request/reply deliberately uses one shared server, so these
scheduler-count results are not a parallel scaling curve.

Precise collection before transfer raised reductions=1 stealing request/reply
throughput by 15.4x and 12.6x over the complete pre-change matrix at two and four
schedulers. The final values are 11.6% and 7.3% below pinned Morrow. The
experimental reductions=32 values are within 2.2% below and 4.6% above pinned,
small differences in this five-round run that do not establish a robust
advantage or justify changing the default. The tuned quantum did not materially
improve lifecycle throughput; stealing remained 3.6x and 3.4x below pinned
Morrow at two and four schedulers.

For contention at two and four schedulers, every run retained all 256 adjacent
probe intervals under load. Median per-run p99 external completion gaps were
0.032/0.028 ms for Morrow stealing reductions=1 and 0.027/0.028 ms for BEAM;
maximum run p99 values were 0.081/0.038 ms and 0.034/0.097 ms respectively.
They are pipe-observed marker gaps, not internal request latencies. Contention
operations/s divides completed probes by the whole scenario interval, including
the hot recurrence, so it also reflects each runtime's integer-work cost. At one
scheduler, BEAM's minimum overlap was only 53 intervals before its hot actor
finished, so that gap result is not a like-for-like full-window comparison.

This remains a workload result, not an overall Morrow/BEAM or OTP parity claim.
The five measured rounds characterize this artifact and host; they are not a
capacity curve or confidence interval.

## Preserved checkpoints and diagnosis

The [complete pre-transfer-GC checkpoint](results/actors-macos-arm64-20260919-before-transfer-gc/)
retains its 216 observations, raw streams, hashes and source manifest. In that
matrix, reductions=1 stealing request/reply reached only 12,379 and 15,311
operations/s at two and four schedulers. It is valid before/after evidence and
is not mixed into the final medians.

The earlier
[`before-fix` attempt](results/actors-macos-arm64-20260919-before-fix/) exposed
root-driver throttling: after a busy poll, the driver waited 10 ms even when its
local queue remained runnable. That attempt stopped at its first 180-second
timeout and is discovery evidence, not a censored benchmark result.

The focused
[`transfer-gc-experiment`](results/actors-macos-arm64-20260919-transfer-gc-experiment/)
alternated the checkpoint artifact with the candidate over 48 bounded
exact-output processes. Its three measured rounds per cell were diagnostic only:
they justified full validation and the fresh matrix rather than replacing it.
The candidate workload hash in that experiment exactly matches the final
measured workload hash.

The accepted implementation collects only after the candidate actor's callback
has returned, when its retained actor control range roots the live continuation,
mailbox and cleanup graph. Collection runs before `transfer` acquires the shared
activity lock and then the actor route lock. The subsequent unchanged
`detach_heap` validation therefore scans the remaining live graph, and collection
finalizers cannot execute while either transport lock is held. The dedicated
runtime regression constructs dead and live payloads, verifies that dead storage
is finalized before donation while the live actor graph survives transfer, and
checks that the finalizer can acquire the activity lock. It failed before the
collection boundary was added and passed with the accepted implementation.

Every complete matrix records compiler, runtime archive, workload, runner,
Elixir and BEAM hashes in `inputs.txt`; `source-manifest.sha256` binds the result
to the relevant Rust sources and Cargo inputs.

## Build and smoke verification

Use a quiet release build for measurements. Compile the workload and harness
before the quiet period:

```sh
cargo build --release -p morrow -p morrow-runtime-native
MORROW_RUNTIME_LIB="$PWD/target/release/libmorrow_runtime_native.a" \
  target/release/morrow build \
  benchmarks/language-comparison/programs/actors.mr \
  -o /tmp/morrow-actor-comparison
mkdir /tmp/morrow-actor-beam
elixirc --warnings-as-errors -o /tmp/morrow-actor-beam \
  benchmarks/language-comparison/programs/actors.ex
rustc --edition=2024 -O -Dwarnings \
  benchmarks/language-comparison/src/actor_comparison.rs \
  -o /tmp/morrow-actor-runner
rustc --edition=2024 --test -Dwarnings \
  benchmarks/language-comparison/src/actor_comparison.rs \
  -o /tmp/morrow-actor-oracles
/tmp/morrow-actor-oracles
/tmp/morrow-actor-runner /tmp/morrow-actor-comparison \
  "$(command -v elixir)" /tmp/morrow-actor-beam \
  /tmp/morrow-actor-smoke --smoke
```

The historical Elixir 1.20.4 / OTP 29.0.6 extraction paths are recorded in
[`toolchain.md`](results/beam-macos-arm64-20260914/toolchain.md), but that `/tmp`
installation is not persistent. Pass an explicit compatible Elixir launcher;
the runner records its version and hashes the launcher, compiled BEAM modules,
binaries and sources.

For the current ARM64 macOS host, `brew fetch --force-bottle erlang elixir`
resolved the same versions to the `arm64_golden_gate` bottles below. They were
checksum-verified and extracted under `/tmp`; no formula was installed or linked.

| Artifact | Official bottle SHA-256 |
| --- | --- |
| Erlang/OTP 29.0.6 | `04ae394dc43f667e0570e24b7affca743fd19d4cb107af76d4fd29c1566b9ba6` |
| Elixir 1.20.4 | `4c56732b58177de6ef0b6790f8c0fcb6673adbd6b2626673be10b091b0bb1368` |

The extracted `elixir` and `elixirc` launchers retain the historical hashes
`5cb23d89a78f75589b06fade3097e217b77e63416d61c1800f674579748c4307`
and `448cc9ffdc2f23604eccfe370076b5afde6069a97e47dcb8d42fe22d1e169ed1`.
The host-specific `beam.smp` hash is
`2a842c593da6fd981913808033e613b7c0d0310cda763beaaf840329a1a02a87`.
Set `ERL_ROOTDIR` to the extracted `erlang/29.0.6/lib/erlang` directory and put
the extracted `erlang/29.0.6/bin` first on `PATH`.

After a successful smoke check, run the same command with a fresh output
directory and without `--smoke`. Do not run builds, tests or interactive desktop
loads during the measured matrix. The output directory is created exclusively so
an earlier result cannot be overwritten.
