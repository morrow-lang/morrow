# Stage 3 opaque values — frozen WIP handoff

2026-09-19. Frozen on explicit user wrap-up; no implementation, builds or tests
may resume without a new instruction. No active child commands remain.
Worktree: `/tmp/morrow-typed-supervision-20260919`, branch task/typed-supervision.
Runtime owner migration_review coordinates the single honest WIP checkpoint.

## Owned changes

- managed/cost.rs: kinds15/16/17 descriptor cases; bounded opaque graph traversal,
  names/token cost, existing shared traversal state; child_spec convenience entry.
- managed/copy.rs: rooted opaque payload deep copies, inert control retention,
  branch arrays, worker frames, Handle identity reuse; child_spec entry.
- NEW managed/supervisor/values.rs: representation and four constructor/id exports,
  helper validation, synchronous transient admission charge, initialized14word Spec.
- NEW managed/supervisor/value_tests.rs: three constructor/authority/GC/quota tests.
- NEW managed/supervisor_value_tests.rs: four independent literal descriptor/layout,
  exact cost, alias/cycle and cross-thread branch fragment regressions.

No other source ownership was modified by this agent. Runtime owner owns module
wiring/constants/identity role/serial issuer/structural validator and the engine.
Compiler owner owns crates/morrow. No commits or broad gates run by this agent.

## Actual evidence (do not overstate)

`/tmp/morrow-supervisor-values-red-20260919.log`: compiled baseline, 4/4 tests FAIL
as expected. kind15 rejected, literal branch cost None vs117, alias graph None
vs157, fragment preflight rejected. An earlier missing-module compile was corrected
with representation-only scaffolding and is not claimed as a behavioral red.

`/tmp/morrow-supervisor-values-first-green-20260919.log`: original4 tests PASS
among6 filtered tests. Hidden graphs and actual branch fragments cross OS threads,
sender text is collected, adopted block addresses remain stable, receiver GC owns
and finally releases its copied payload. No worker/PID extension in this run.

`/tmp/morrow-supervisor-values-constructors-20260919.log`: three constructor tests,
2pass/1fail. The quota test independently caught a real new-code bug: eager
Option::then_some constructed and dropped Charge after rejected charge, releasing
44 bytes that were never reserved. Changed to lazy then; unchanged quota oracle.

`/tmp/morrow-supervisor-values-constructors-green-20260919.log`: 3/3 PASS. Covers
key authority/name deep copy/source collection/epoch token survival after close;
worker wide Int/negative-zero Float/String alias/JSON deep copy under injected GC,
original-template independence from mutated copied worker state; rejected key and
invalid options leave logical accounting unchanged.

After that passing run, the worker test was extended to PID+ProcessId captures and
an actual cross-OS-thread Fragment after session close. This extension has NOT RUN.
`/tmp/morrow-supervisor-values-fragment-20260919.log` is the final command, exit101:
values.rs calls validate_specs(s, child_slice, auto_shutdown), but supervisor.rs
still exposes the previous two-argument scaffold. E0061 stops compilation.
No fixes or new executions were launched after wrap-up.

Commands used target `/tmp/morrow-typed-process-model-target-20260919`:
`rtk env CARGO_TARGET_DIR=... cargo test -p morrow-runtime --lib supervisor_value_tests -- --nocapture`
`rtk env CARGO_TARGET_DIR=... cargo test -p morrow-runtime --lib supervisor_ -- --nocapture`
`rtk env CARGO_TARGET_DIR=... cargo test -p morrow-runtime --lib supervisor::values::tests -- --nocapture`
Owned files formatted with rustfmt edition2024 before final attempted build.

## Known pending integration / review

1. Runtime owner must implement agreed validate_specs(s, specs, auto_shutdown),
   then remove its deliberate rejection scaffold using its own structural reds.
   Per-path depth64 including the virtual root, alias DAGs, cycles, sibling names,
   child count1024 and parent auto-shutdown/significance remain its ownership.
2. Run the newly extended worker/PID/ProcessId/JSON cross-thread test; its success
   is not established. Rerun all constructor and descriptor tests after signature
   integration. Fresh compile may expose additional issues masked by E0061.
3. Runtime owner should re-export four values functions from managed.rs for Rust
   public-ABI integrations; no_mangle native symbols already exist.
4. More independent coverage remains: malformed schema/tag vs ordinary option
   errors, foreign invocation, exact mailbox mismatch, branch constructor success,
   name boundary4096, near-quota worker rollback, larger descriptor/work/depth
   limits, forced GC at each partial-copy allocation, whole-heap migration of these
   values. Existing root-slot/runtime-agent tests are separate evidence.
5. Full runtime tests, strict runtime + external-Exec ABI Clippy, compiler native
   context tests and parent fullgate/TSan still required. Current warnings included
   runtime-owner pending native_root_slot usage; do not suppress these blindly.
6. ChildSpec/KeyToken graph charge includes copied name and fixed24byte token
   footprint. Physical token storage is counted once by Owned. Constructor charge
   is transient owner-local; long-lived frame/message/template admission charges
   the graph afterward. No Session/Shared in token and no foreign owner-local Drop.
7. Generic cost uses aggregate existing work/bytes/depth/memo. Full structural
   depth64 cannot rely on cost memo: the shared engine validator is mandatory.

Accepted exact contract: `/tmp/morrow-stage3-opaque-values-contract-20260919.md`.
It describes intended invariants, not completed engine or acceptance evidence.

## Deferred memo-only candidate, preserve independently

Worktree `/tmp/morrow-copy-memo-only-20260919`, based cafc91d.
Patch `/tmp/morrow-copy-memo-only-20260919.patch` SHA256
fd0b9d311d9bc407111f5bdbfe49585970479c2f6eecd2100c9efec7c764479e.
Evidence `/tmp/morrow-copy-memo-only-evidence-20260919/README.md` and sibling logs.
Safe2entry exact(source,descriptor) memo, unchanged HashMap overflow and JSON map;
production copy dispatch including ProcessId/MonitorRef untouched. Fresh oldsource
6tests:4pass2fail; candidate6pass. Tiny graph metadata2alloc916bytes→1alloc808bytes,
payload blocks unchanged. Runtime236unit+19integration=255distinct; native Process
2tests×6configs=12executions; strict Clippy passed. No timing claim or adoption.
Parent was asked to archive this patch/evidence durably; do not mix it into stage3
production changes or imply the deferred combined tiny-fragment candidate passed
performance acceptance (it did not).
