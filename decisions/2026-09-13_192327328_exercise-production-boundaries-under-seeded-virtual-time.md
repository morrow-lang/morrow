+++
schema_version = 1
id = "01M2XHZ870FQ0M9K6K7N1XMS9J"
title = "Exercise production boundaries under seeded virtual time"
date = "2026-09-13"
status = "accepted"
tags = ["architecture"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; final integrated acceptance is recorded in the roadmap
* **Decision**: Add an opt-in, invocation-local virtual clock to the native managed runtime and a Rust `fern-sim` package. Drive the production protocol/client state machine and compiled Fern room actors with an ordered, seeded fault schedule, real local checkpoint reopen, independent state checks and a fault-free convergence phase. Separately drive native callback ABI scenarios for selective receive deadlines, sibling progress, supervision, churn and precise collection. Expose both through `cargo xtask simulate` with versioned JSON reports and exact replay.
* **Context**: Studying pinned Phoenix LiveView, Erlang/OTP and TigerBeetle sources clarified three useful boundaries: local interaction versus confirmed server state, callback budgets versus general preemption, and simulated event coverage versus elapsed production time. Fern can exercise its own implementation now without claiming a replicated VM or replacing real browser/OS acceptance.
* **Consequences**: Bounds apply to workload, clients, rooms, event queues and retained trace independently of virtual duration. Reports exclude wall time, paths and native addresses. Simulation is explicitly enabled; package-specific web builds retain the real clock, while workspace feature unification can include dormant simulation state. Failures retain replay configuration and fail the command. The initial campaign exposed an empty decoded-list capture invariant during durable restart; an independent add/remove/reopen regression protects that behavior. A separate forced-collection regression protects session construction roots. Typed JSON constructors now root partially built values across allocation; actor map transfer follows the compiler’s untagged key/value pair ABI, protected by both native descriptor and compiled Fern oracles. Arbitrary preemption, replicated ownership, disk power-loss simulation and complete precise native layouts remain separate work. Source studies and a three-part runnable demo are linked from `docs/DETERMINISTIC_SIMULATION.md`.
