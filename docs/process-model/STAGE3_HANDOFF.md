# Stage 3 WIP handoff — 2026-09-19

Stopped at the user's wrap-up request. This is an **incomplete checkpoint**, not
stage 3 acceptance. All three owners froze their files and reaped their commands
before checkpointing. Do not infer that this tree compiles from earlier focused
greens.

Base: stage 2 commit `7166d08172ce606ffc47bbc30ebfcfe5e0a4ad7e`.
Branch: `task/typed-supervision`. Worktree:
`/tmp/morrow-typed-supervision-20260919`. Shared Cargo target:
`/tmp/morrow-typed-process-model-target-20260919`.
Do not rebuild the old stage 2 worktree into that target concurrently.

## Immediate blocker and incomplete runtime

The latest runtime command exited101: `supervisor/values.rs` calls the agreed
`validate_specs(session, children, auto_shutdown)`, while `supervisor.rs` still
contains a **two-argument, always-error scaffold**. This is E0061; the new
PID/ProcessId cross-thread fragment extension has not executed. Preserve the
existing assertions while implementing the validator; do not merely remove the
third argument to make the tree compile.

`morrow_supervisor_register` is also an explicit red-test scaffold: it always sets
fault11 and returns3. No registration set, owner-close teardown, request engine,
reply adoption, startup acknowledgement or restart policy has been implemented.
The other request/startup runtime symbols required by generated code are absent.
The four synchronous constructor/id symbols exist, but their public Rust
re-exports in managed.rs remain to be added.

Actor still has six payload roots. Its seventh reply root, exact offset/root
oracles, request ticket state and reply handoff are pending. For callback authority,
reuse main's `Actor.running_function:*const Function`; save/set/restore around
nested selectors and defers so they cannot inherit an outer resume authority.

## Saved runtime work

- `memory/heaps.rs`: checked reply-slot membership in the current owner's top
  native frame. Persistent roots, wrong owners/domains, earlier frames, empty
  frames, unaligned/out-of-bounds/overflow slots reject. This predicate is not yet
  connected to take_reply.
- `supervisor.rs`, `relations.rs`, managed.rs: immutable supervisor-role bit and
  read-only role check; fresh checked nonzero key serials under the retained
  invocation epoch. Tokens remain inert after close. No controller currently
  publishes the supervisor role in production.
- `supervisor/registration.rs`: frozen repr(C) four-word registration record plus
  independent failing tests for canonical acceptance, idempotence, no prefix
  publication, metadata release despite retained PID, and registration on both
  sides of parallel configuration. Actual registration behavior remains absent.
- `supervisor/values.rs`, cost.rs, copy.rs and dedicated tests: kinds15 Handle,
  16 ChildKey(M),17 ChildSpec; constructor/id functions; bounded header/option
  validation; shared traversal/memo for hidden template graphs and fragments.
  An eager `then_some` temporary-charge rollback defect was caught and fixed.

Opaque layout is frozen: Handle wraps retained ProcessId identity with immutable
role validation. ChildKey has token+copied native name; its inert token holds
Arc<Epoch>, serial and mailbox, never Session or payload pointers. ChildSpec is14
initialized words: kind, key, name, initializer, children, children_len, restart,
shutdown_kind, shutdown_ms, significant, strategy, intensity, period_seconds,
auto_shutdown. Inactive fields are zero. Templates are copied into the supervisor
heap; a foreign payload heap must never become shared template ownership.

Shared structural validation must check every alias path, cycles,1024 siblings,
unique sibling names, and64 supervisor levels under one work bound. Virtual/current
supervisor is depth1; Branch increments, Worker does not. Significant children
require their **parent's** auto_shutdown to be enabled; Permanent+significant is
invalid. Return internal `ValidationError::Options(tag)` versus `Malformed`, so
infrastructure fault11 cannot be confused with Error.StartFailed(tag11).

## Saved compiler work and known gaps

`crates/morrow` contains schemas/opaque types, Root versus Actor(mailbox)
specialization, sealed RootFunction and IR root_context, constructor ABI/startup
Never lowering, private request records/CPS suspension/rooted take_reply, and
CLI/library registration publication. These are WIP and have no native supervisor
integration acceptance.

The compiler owner identified these follow-ups before freezing:

- Treat Supervisor.worker's entry as a direct-spawn boundary in capture validation;
  otherwise captured RootFunction escape may evade the source restriction.
- Reject bound RootFunction invocation in defer as well as named contextual helpers.
- Validate root_context against forged public IR; reject internal markers in source
  AST preflight; audit wrapped type walkers and callable Result obligations.
- Add checked fault15 diagnostic mapping.
- Fix the unexecuted delayed_ack draft: legacy send returns Result(Unit,Int), while
  its current handled helper expects Process.Error.

## Actual evidence, not whole-tree acceptance

The existing runtime logs and both owner contracts are archived under
[evidence/stage3-wip-20260919](evidence/stage3-wip-20260919/README.md). This archive
was copied at wrap-up without rerunning tests.

- Native root-slot predicate: one behavioral red then one green.
  `/tmp/morrow-stage3-native-root-slot-{red,green}.log`.
- Key/role helpers: two behavioral reds then two greens.
  `/tmp/morrow-stage3-key-role-{red,green}.log`.
- Registration: three expected behavioral failures, still unresolved.
  `/tmp/morrow-stage3-registration-red.log`.
- Opaque graph/descriptor/fragment slice: four baseline failures then four greens.
  `/tmp/morrow-supervisor-values-red-20260919.log` and
  `/tmp/morrow-supervisor-values-first-green-20260919.log`.
- Constructors: two passed and one quota rollback failure, then three passed after
  correction. `/tmp/morrow-supervisor-values-constructors-20260919.log` and
  `/tmp/morrow-supervisor-values-constructors-green-20260919.log`.
- Latest extended fragment attempt **did not compile**:
  `/tmp/morrow-supervisor-values-fragment-20260919.log`.
- Compiler `supervisor_context`: last completed run5/5 green, including Cranelift
  object emission. Earlier RTK red records: `bb7da50b36a8`, `3ff354680786`,
  `93b32dd25b44`. Subsequent static Result/defer/public-IR edits were not rerun.

No stage3 formatting/Clippy/full gate, TSan, actor replay or source-native scheduler
matrix has passed. Stage2's accepted evidence belongs to its base commit only.

## Resume sequence

1. Resolve the shared structural validator contract with independent valid/depth65,
   aliased-DAG, cycle, duplicate-name and parent-policy oracles; run the pending
   opaque fragment extension. Preserve all existing failing assertions.
2. Implement all-or-nothing immutable registration and explicit owner teardown after
   worker joins, even when retained PIDs keep Registry alive. See
   [registration ABI](supervisor-registration-abi.md). No GC/callback under locks.
3. Add the seventh reply root, exact callback/resume and native output-slot authority,
   prepaid request/reply ownership, cancellation/late-reply cleanup and GC handoff.
4. Implement static OneForOne startup/ack/current/stop and actual Permanent,
   Transient,Temporary restart behavior plus inclusive rolling whole-second
   intensity: intensity0, initial startup uncharged, failed restart attempts charged,
   fresh generations, legacy unchanged. Grouped/nested strategies and dynamic
   management remain later stages.
5. Complete the compiler boundary fixes, then independent source/native
   S1/S2/S4 × stealing off/on fixtures, focused GC/race/quota tests and full gates.

The accepted [compiler contract](supervisor-compiler-contract.md),
[context map](supervisor-context-map.md) and
[overall process specification](../superpowers/specs/2026-09-19-typed-otp-process-model.md)
remain the design requirements. This checkpoint does not establish their implementation.
