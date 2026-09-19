# Actor parity handoff — 2026-09-19

Paused at the user's request to conserve tokens. All work is being committed and
pushed. Do not claim performance parity or complete OTP supervision.

## Repository and branches

- `main`: accepted performance checkpoints, isolated processes/monitors, and the
  integrated links/exit stage. Read `CLAUDE.md`, `ROADMAP.md`, `DECISIONS.md` and
  `docs/PROCESS_MODEL.md` first. Every shell command must use `rtk`.
- `task/typed-supervision`: explicitly unfinished stage3 checkpoint. Its tracked
  handoff records the exact state. **It does not compile at pause:** values.rs
  calls `validate_specs(session, children, auto_shutdown)` while the runtime
  scaffold still takes two arguments. Do not merge or call it accepted.
- Existing stage3 worktree: `/tmp/morrow-typed-supervision-20260919`, shared target
  `/tmp/morrow-typed-process-model-target-20260919`. All three agents used this
  source worktree; avoid rebuilding the older stage2 source into the same target.

## Current acceptance and the immediate verification step

Stage2 standalone commit `7166d081` passed the complete gate, TSan247, exact
actor replay and30 native scheduler configurations. Main integration preserves
private frame reuse, descriptor caching, scalar send outcomes and exact tail
batching. Its focused compiler/native checks and full runtime pass; integrated
TSan passes263 tests (two long churn cases covered normally), and actor replay
remains `d4e402a412f11e2f`,48739 callbacks, zero residue.

The latest **main full gate is incomplete**, stopped by an environmental linker
failure in `sole_backend::default_native_compilation_does_not_resolve_qbe`.
The selected linker cannot read `arm64e.x1` entries in the installed MacOSX27.0
SDK. Both that three-test suite and a release workload link pass when selecting
the installed26.5 SDK. No system configuration or compiler source was changed.
Preserved evidence is in [continuation-evidence-20260919](continuation-evidence-20260919/).
On resumption, run this first and record the integrated full result:

```
rtk env SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo xtask check
```

The first focused integration attempt also linked the stale pre-stage2 archive;
rebuilding `morrow-runtime` and rerunning the actual native fixtures passed.
That setup failure is not a source semantic regression. Do not relabel either
failed invocation as a passing full gate.

## Performance state

Latest accepted matrix: [checkpoint4](../benchmarks/language-comparison/results/actors-parity-checkpoint4-20260919/PARITY.md).
All216 observations and independent validations are retained. Strict and
competitive acceptance are both **0/9**. Default stealing/reductions1 ratios:

| Workload | S1 | S2 | S4 |
| --- | ---: | ---: | ---: |
| Request/reply | .304 | .095 | .158 |
| Contention | .785 | .373 | .398 |
| Lifecycle | .445 | .119 | .126 |

The independent send-outcome comparison improves all12 medians1.042–1.292×.
Exact tail batching improves all6 contention medians1.439–1.460× within the
unchanged32-unit work budget. Checkpoint4 nevertheless retains an18.8% S4
request/reply regression versus checkpoint3; no cause has been established.
The nine-round BEAM confirmation is ineligible until every strict cell>=1.00.

The combined tiny-fragment/memo change was deferred after repeated unfavorable
aggregate comparisons. Its exact patch and both raw diagnostics are committed.
A separate [memo-only candidate](../benchmarks/language-comparison/experiments/20260919-copy-memo/README.md)
removes one temporary allocation/108 bytes per tiny copy, with focused/native
checks; it remains unmeasured and unapplied. Do not claim that allocation saving
is a throughput improvement.

Next proposed runtime experiment (not implemented): shared `drain_actor` now
locks its route at every callback/selector/cleanup safe point even when empty.
An Ingress atomic pending hint could skip empty locks. It must be set under the
route lock in BOTH publication helpers and cleared under that same lock when
queues are taken/discarded. Preserve notification, stop, migration and first-fault
ordering with independent lock-count and barrier tests. The supervisor agent
confirmed future actor ingress payloads use those same publication helpers.

## Supervisor work remaining

Stage3 must implement actual static OneForOne startup acknowledgement/current/
stop, Permanent/Transient/Temporary selection, inclusive rolling intensity,
intensity0, failed restart charging, fresh generations and cancellation rollback.
Root callers may drive polling; actor callers must suspend, including transitive
helpers and lifted closures. Stage4 adds grouped strategies and nested ordered
shutdown; stage5 adds dynamic/significant-child policies. Their exact accepted
contracts are linked from the [specification](superpowers/specs/2026-09-19-typed-otp-process-model.md).

WIP acceptance is limited to focused root-slot, identity, value-graph/constructor
and compiler-context tests. Registration/structural validation still has failing
scaffolds; actual supervisor engine/RPC/native integration and complete gates
remain outstanding. The branch handoff lists compiler escape/defer/public-IR
validation gaps and untested later edits. Fix the compile blocker first, then
continue red-first; do not weaken the pending tests.

Use existing Actor.running_function for callback authority, also preserving
main's frame reuse. Save/set/restore around selectors and cleanup callbacks so
they cannot impersonate an outer resume function. Reply storage must be a seventh
explicit Actor payload root. Registration metadata needs explicit owner close
after joins even when inert PIDs retain the registry.

## Useful frozen local artifacts

Checkpoint4: `/tmp/morrow-actors-tail-batch-exact-20260919`,
`/tmp/libmorrow-tail-batch-exact-20260919.a`,
`/tmp/morrow-compiler-tail-batch-exact-20260919`. Hashes are in the committed report.
The integrated links baseline was built as
`/tmp/morrow-actors-links-baseline-20260919` using SDKROOT26.5, with archive/compiler
and337-input manifest frozen under `/tmp/*links-baseline*`. It has no accepted
new timing or completed final smoke at pause.

Quiet comparisons require all agent builds/tests and other benchmark processes
to stop. Preserve every sample, source hash, semantic oracle, fault and SDK failure.
Main and the supervisor checkpoint branch are separate continuation points.
