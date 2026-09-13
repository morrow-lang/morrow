# Deterministic simulation and a Fern demo

Fern's simulation tools run the real native actor scheduler and compiled Fern
application under seeded event schedules and virtual time. They exercise
implementation behavior, with independent expected-state checks. They do not
turn simulated days into a claim of production maturity or complete coverage.

## Run a scenario

From the repository with its pinned Rust toolchain:

```sh
cargo xtask simulate --seed 42 --steps 3000 --days 30
cargo xtask simulate --actors --seed 42 --steps 5000
```

The application scenario runs the production command protocol, client state
machine and native Fern room actors. It delays, drops and duplicates wire
messages, disconnects clients, expires namespaces and restarts the server.
Durable mode uses actual private room checkpoints. Each session receives virtual
time before native calls; no simulated timer needs a wall-clock sleep.

The actor scenario drives the real managed scheduler through messages, receive
deadlines, bounded polling, yielding siblings, checked faults, supervision and
actor churn. It checks output values and cleanup independently of the callbacks.
The [actor simulation contract](ACTOR_SIMULATION.md) records its exact scope.

Save and replay an application run:

```sh
cargo xtask simulate --seed 42 --steps 3000 --days 30 --json > scenario.json
cargo xtask simulate --replay scenario.json
```

Actor reports support the same JSON/replay workflow. Reports keep their seed,
configuration, simulator version and deterministic results. Preserve the Fern
source commit alongside a report: replay checks results against the current
implementation, and a later implementation can intentionally change behavior.
Wall-clock measurements and native addresses are excluded from replay identity.
Use `cargo xtask simulate --help` for fault probabilities and resource bounds.

## What the application checker establishes

An independent map-based state model checks each compiled Fern transition and
restored room. Additional invariants check that one command cannot commit twice
in its namespace, rejected/duplicate commands cannot change state, revisions
advance correctly, stale snapshots cannot move clients backwards, and network
events cannot alter unsent local drafts. The event queue and retained trace are
bounded independently of the requested virtual duration.

Every chaos run ends with fault injection disabled. Clients must reconnect,
resolve uncertain commands, make a new successful mutation in each room and
converge on the authoritative state. This checks progress after disruption as
well as safety during it. A deliberate test-only corruption verifies that the
independent checker detects a bad native result.

The modeled network connects clients to one server. It is not a replicated
cluster or consensus implementation. Checkpoint reads/writes use the actual local
filesystem; arbitrary disk corruption, torn writes and physical power loss are
not simulated. Authentication transport, OS scheduling and browser execution keep
their separate real integration tests. Increasing `--days` changes event times;
increasing `--steps` changes how much workload is exercised. A sparse month is not
equivalent to a busy production month.

Simulation is an opt-in Cargo feature. The package-specific `cargo xtask web-build`
does not enable it. Workspace-wide builds can unify the simulator's feature into
shared runtime artifacts; ordinary native domains still use real time unless an
explicit simulation clock is supplied.

## Measured development scenarios

On 2026-09-13, macOS ARM64 with the pinned toolchain:

| Scenario | Work exercised | Result |
| --- | --- | --- |
| Seed `0xc0ffee`, 3,000 decisions over 30 virtual days, default durable faults | 12,283 events; 2,039 native commits; 28 server restarts; 56 room recoveries | Passed and replayed byte-for-byte |
| Seed `42`, 1,000 decisions, 16 clients in one room, forced duplication, no delay/drop/restart, ephemeral | 71,018 events; 727 native commits; 68,067 checked snapshots | Passed in 11.409 seconds in a debug build |
| Native actors, seed `0xc0ffee`, 5,000 rounds, optimized build | 48,362 scheduler turns; 2,434 restarts; 10,037 churn actors; 1,685,305,546 virtual milliseconds | Passed in 1.22 seconds; exact replay in 0.77 seconds; final managed counts zero |
| Native actors, seed `0xc0ffee`, 100,000 rounds, optimized build | 965,734 scheduler turns; 50,051 restarts; 199,769 churn actors; 390.7 virtual days | Passed in 18.27 seconds; exact replay in 17.90 seconds; final managed counts zero |

These timings are individual observations under shared host load, not throughput budgets.
Neither count estimates production years. To reproduce its workload:

```sh
cargo xtask simulate --seed 42 --steps 1000 --clients 16 --rooms 1 \
  --duplicate-per-mille 1000 --delay-per-mille 0 --drop-per-mille 0 \
  --disconnect-per-mille 0 --restart-per-mille 0 --delay-ms 0 --ephemeral
```

The initial durable-restart campaign found a real defect: restoring a room after
removing its final task decoded an empty list that actor capture rejected.
An independent native add/remove/reopen regression now checks empty recovery and
continued non-reused task identities. Related ABI tests cover decoded map capture
and literal map messages with Unicode keys and full-width integers. These are
examples of bugs found and protected, rather than an inference from a replay hash.

## Demo the language in three parts

1. Run `cargo xtask build`, then `./bin/fern run examples/supervised_workers.fn`.
   This intentionally faults one supervised native worker, runs its cleanup on
   each attempt and lets a sibling finish. There are two restarts, then that
   lineage stops. The source uses typed PIDs and automatic memory; ordinary
   recoverable application errors should use `Result`.
2. Follow the [web build guide](WEB_PREVIEW.md), set `FERN_WEB_DATA_DIR` to retain
   room state, and open two browser windows. Type into the local preview, submit
   a task and observe server confirmation in both windows. Disconnect one
   browser using its developer tools: draft preview, byte accounting and filters
   keep executing in Fern WASM. Reload after the application has cached, then
   reconnect. Shared mutations require a connection.
3. Run and replay the seeded scenarios above. Change the seed, workload and fault
   settings; inspect the event counters and trace digest. This demonstrates a
   reproducible development method alongside the executable language.

The [shared Fern application](../examples/web/checklist.fn) owns model/update/view
and domain behavior. The [server adapter](../examples/web/server.fn) owns typed
room actors. Rust supplies generic DOM operations, host capabilities and
transport. Application-independent framework packaging, general preemption,
complete precise native layouts and distributed ownership remain open.

## Integrated acceptance

The final macOS ARM64 `cargo xtask check` passed formatting, dependency notices,
workspace Clippy, 1,924 Rust tests across 254 suites, 305 native-output fixtures,
19 examples, 63 dynamic compatibility programs, 295 atomic rejections and
64 grammar plus 192 mutation fuzz cases. No oracle was skipped or weakened.
This includes the independent typed-decoder collection, compiled map transfer,
empty checkpoint recovery, virtual-clock and failure-sentry regressions.

Code checkpoint `3be4025be551758fee4f0e250e152c137a83890e` also passed 138
optimized tests on actual ARM64 Linux: 84 core runtime/simulation tests, eight
checkpoint/host-clock tests, 17 simulator tests and all 29 native backend
oracles. The macOS application and actor reports replayed byte-for-byte on Linux.
The checksum-matched static ARM64 server passed the complete browser suite;
desktop and mobile screenshots were inspected and the validation VM was stopped.
x86-64 has static binary validation here; GitHub browser execution is tracked
separately from this local ARM64 acceptance.

The CI follow-up fixes two independently reproduced harness defects: opener
failure configuration no longer depends on the platform's `argv[0]`, and browser
screenshots activate their target and await painted readiness under one unchanged
15-second budget. The final local gate passed 1,927 Rust tests across 254 suites
and the same full native/example/compatibility/fuzz tail. Updated real-browser
acceptance passed from an observed hidden page, with valid PNGs and visual checks.
GitHub Actions execution remains a separate result from these local checks.

## Demo artifacts

The final 2026-09-13 resilience build embeds asset revision
`f2c2063bfde6fe96e45b1832c566d25e0bb7d054af619163f9ea4b28a7e0b0f0`.
The macOS build passed the full real-browser suite and desktop/320-pixel mobile
visual inspection, including the new draft preview and pending feedback.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| macOS ARM64 | 2,866,992 | `29cc474508dfca2e4be04e8b50f50fb2e457c119d70ae79b425c8cc2a64f7a4a` |
| Linux ARM64 musl | 2,891,952 | `2589058731ad4e8519305ed08b1cdfbcf39c339fb78925cf049f4f994d93d341` |
| Linux x86-64 musl | 3,214,552 | `9c84442ff21396cc2abe3a59e9a615f7098393ac9231265ccb54efbd5b27cd4f` |

Separately, `examples/tiny_cli.fn` linked against the optimized native runtime
produced a 566,664-byte macOS ARM64 executable and printed `hello, fern`. It links
only platform `libSystem` and `libiconv`, with no browser or web-server bundle.

Both Linux builds passed the static ELF checks: no interpreter or required shared
libraries. These sizes include the checklist server, browser assets and dependency
notices. They are application measurements, not the size of the compiler or a
minimum CLI. Public HTTPS/WSS still needs a TLS terminator. The earlier
[application/worker acceptance](WEB_APPLICATION_ACCEPTANCE.md) remains a separate
historical record with its own source and artifact hashes.

## Source inspiration

[TigerBeetle's VOPR documentation](https://github.com/tigerbeetle/tigerbeetle/blob/47aeb2212a255273dda508288412e537d11e4b7c/docs/internals/vopr.md)
and [architecture](https://github.com/tigerbeetle/tigerbeetle/blob/47aeb2212a255273dda508288412e537d11e4b7c/docs/ARCHITECTURE.md)
informed the fixed-seed event generator, controlled boundaries, independent
checker and healthy recovery phase. This is Fern's implementation; no TigerBeetle
code was copied, and Fern does not claim its consensus/storage coverage.

The [LiveView source study](LIVEVIEW_STUDY.md) informed scoped pending feedback
and preservation of local input. The Erlang/OTP source study in the actor
simulation contract distinguishes scheduling reductions and rolling restart
intensity from Fern's current callback budgets and lifetime restart limits.
