# Stage2 runtime implementation and evidence

Worktree: /tmp/morrow-typed-process-model-20260919. Base stage1:73afafc. Runtime source owner migration_review; compiler source owner bounded_tail_batches; root owns docs/gates. No stage2 commit made by runtime agent.

## Implemented

Six additive native symbols: morrow_process_link, morrow_process_unlink, morrow_process_spawn_link, morrow_process_trap_exit, morrow_process_exit, morrow_process_signal_exit. Exec/Type/Function public layouts unchanged; ProcessId/MonitorRef remain kinds13/14. Message kind is now full-word u64 through independently accepted main compatibility correction. Actor GC root registration uses precisely six contiguous pointer fields through the independent main correction.

New controls/reasons/links/actions modules separate prepaid directed capacity, immutable reasons, symmetric link epochs, and bounded completion publication. Transport payload is an enum. Pair admission reserves both endpoints before either publishes; remote owner mirrors are only installed/released on the owner. Cancellation and signal conversion share a token mutex, and an old epoch stays retained through pending completion independently of the active pair index. Already queued Exit survives unlink. Repeated unlink issues at most one release per token. Dead-link completion has its own cancelable epoch.

Future monitor/link slot admission is512+4097 bytes,256 slots per recipient/4096 global; direct known causes use512+actual NUL-inclusive text size. Actual immutable retained text has its own bounded charge, up to4096 UTF-8 bytes. Local admission includes a separately retained atomic reason counter; foreign final Arc destruction never dereferences Session. Shared Budget and accounting counters own their physical accounting lifetime.

Exit commits before return but retires only after native callback return. Direct Kill can escalate cleanup suppression at the next safe point without overwriting an earlier committed reason; currently running callback/defer completes. Local or linked Kill does not force this policy. Cleanup imports controls between callbacks. Ordinary ScopeLeave returns additive status2 on terminal intent, which the paired compiler change returns through its normal rooted epilogue before constructing a continuation. Checked faults and malformed infrastructure retain distinct policy.

Completion actions publish at most64 per owner turn, never recursively retire targets, and keep global pending-work credit until publication/disposal. Linked Exits precede Downs from the same retirement, preserving the independently pinned Down/unlink observation even across a64-action boundary. Stop drains actions and every ledger. Exiting rejects new user messages while monitor/link registration before retirement receives the committed cause. Timer/selector safe points precede subsequent callbacks; selected native frames receive an explicit root across ingress adoption.

## Red to green observations

- Directed slot primitives were initially absent; pair rollback and real-owner mirror tests pass.
- Monitor unknown-text prepayment: old actual512 vs required4609 failed, then exact4609 admission and512 builtin materialization passed. The4096-active-monitor oracle changed only for this explicitly accepted policy; queued builtin oracle remains512 each.
- Local exit/direct signal/link entry points initially absent; independent native terminal and symmetric link tests pass.
- Pre-envelope clock failure leaked512 bytes (actual525408 vs baseline524896); direct Exit and Install rollback now pass.
- Kill after committed intent ran one defer instead of zero; escalation now preserves cause and suppresses pending defers, including deterministic real-worker before/during-cleanup barriers.
- Completion fanout published65 instead of64 in one turn; bounded queue and sibling progress test now pass.
- Ordinary ScopeLeave returned0 rather than terminal2; independent native runtime oracle now returns2 with no remaining forced-cleanup scopes. Paired compiler oracle independently verifies zero resumed source/successor publication.
- Down at a64-action boundary preceded still-cancelable linked Exit; independent completion barrier assertion failed, then Exits-before-Down ordering passed.
- Exiting accepted a user message (Ok tag0 instead of Err1); identity/publication checks now reject it while real cleanup-barrier monitor/link registration remains valid.
- Due timer path called one selector after terminal Kill instead of zero; safe points before timer/selectors now pass.

Test harness corrections were distinct from runtime bugs: large ring fixture needed an early fixed root range for IDs during construction; shared-budget cleanup expectations preserve the existing initial invocation charge rather than expecting zero; parallel poll may return budget status2 while an unrelated worker is still reporting idle, so last-work test waits boundedly for done after verifying immediate queue release.

## Validation actually run

- Full cargo test -p morrow-runtime:243 library tests plus24 integration/protocol tests, all267 passed,65.91s library time. This was before three final test-only additions (stop publication, quota cause, malformed cause). All production fixes were included.
- Current dedicated link_tests:20 passed before the final two reason-mapping cases; those two separately passed in the reason_ subset (8 total matching tests). Current library inventory246. Final all22 link tests passed (1.13s), and runtime all-target Clippy passed after those final test additions.
- Earlier managed subset136 passed; all24 stage1 process tests preserved after explicit action-service call in the newly added stage2 monitor-prepayment test.
- New foundations:controls2 and reasons3 passed, including foreign post-close final Arc release and exact64MiB combined local admission.
- Runtime all-target Clippy with -D warnings passed; rerun after final test-only additions also passed. cargo fmt --all run.
- Initial immutable archive: compiler reports stage2 source fixtures12 runs + stage1 fixtures12 runs green across S1/S2/S4 x stealing off/on with precise owner-GC. ScopeLeave compiler hook expanded preliminary matrix to30 green runs. Final refined archive rebuilt; compiler confirmed all30 runs green using immutable archive SHA256 ddf017286044074a4a0f2f599ece502f3cdd8fda4389bd4798e501530ac99e21.
- Full workspace gate started at /tmp/morrow-process-stage2-check.log after runtime freeze. TSan and replay remain pending; no stage2 acceptance claim yet.

## Dedicated coverage

Real opposite-worker link/unlink deduplication; real Exiting registration barrier; real Kill-before-first-defer and Kill-during-first-defer;256 blocked remote install/unlink churn with exact owner mirrors and stop/adoption cleanup;4096 completion fanout with64-action cadence in local/shared modes and stop midway; last-live retirement pending work;128-actor ring and32-actor fully connected cascade with full-width first cause; atomic spawn_link admission rollback/no child body; old pending epoch cancellation/relink and queued Exit preservation; migration carrying queued Exit, adopted link, in-transit new link install/direct signal and message order with precise GC; stop after actual admission before route publication; malformed/oversize/capacity reason mapping; forced intermediate GC during Event.Exit materialization. Existing monitor, transport, migration, ABI, native root and legacy tests remain active.

## Deliberate limits

Cooperative native callbacks/defers are not preempted. No distributed identities, priority kill lane, arbitrary OTP standard library or hot-code behavior. Stage2 is the local typed process signal substrate; typed OTP supervisors are stage3 and not implemented here. No new overall BEAM performance equivalence claim follows from these tests.

## Checked-failure precedence correction (final gates pending)

The pre-correction full xtask gate was interrupted intentionally after a confirmed precedence defect; its log is not passing acceptance evidence. Pre-correction TSan and replay remain historical green results only.

Three real-worker barrier oracles in `managed/fault_signal_tests.rs` were recorded red in `/tmp/morrow-stage2-fault-precedence-three-red.log`: both checked callback Fault5 and first cleanup Fault5 became Killed when a later admitted direct Kill was processed; the supervised legacy child ran once instead of restarting (expected two callbacks). The cleanup oracle blocks a subsequent successful defer while Actor.fault is temporarily reset to zero.

Correction: the terminal record distinguishes a latched checked cause from explicit terminal intent. Checked causes are latched before post-callback and selector ingress and before/between cleanup callbacks. Later direct Kill may force remaining cleanup suppression without replacing the cause. Checked-origin retirement still follows existing isolated/legacy/supervision policy. Explicit Process.exit(Fault(code)) remains explicit terminal intent.

All three tests green in `/tmp/morrow-stage2-fault-precedence-green.log`, covering callback/cleanup × isolated/legacy × Kill/Failure, plus legacy supervised recovery for both signal reasons. No failing oracle was weakened. Full post-correction validation is pending.

### Corrected-tree completed validation

- `cargo test -p morrow-runtime`: 249 library +24 integration/protocol tests pass, including both long churn regressions. `/tmp/morrow-stage2-fault-precedence-runtime.log`.
- Runtime all-target Clippy with `-D warnings`, formatting: green. `/tmp/morrow-stage2-fault-precedence-clippy.log`.
- TSan:247 passed,2 long churn tests filtered (ordinary green),17.99s. `/tmp/morrow-process-stage2-tsan-final.log`.
- Actor replay exact: seed4608590/5000 steps;48739 callbacks,1246 delivered,3754 timeouts,2507 restarts,10000 churn;cleanup all zero;hash `d4e402a412f11e2f`. `/tmp/morrow-process-stage2-replay-final.log`.
- Corrected-origin native matrix: all30 subprocess runs green (five fixtures,S1/S2/S4,stealing0/1,forced owner preciseGC). Immutable archive SHA256 `287ea044e0766aacdab20a5927d791d8348d5f2b90a32dbdf750e27c12a5ca84`, `/var/folders/n1/d2x5svbs71dd5xvfqyzc2qcr0000gn/T/morrow-stage2-origin-native-20260919-mhrbrol1/libmorrow_runtime.a`.
- Full xtask check remains running at time of this update. Source frozen.

Coverage caveat retained: trap flag toggling and simultaneous endpoint death have semantic/cascade coverage but no separate dedicated two-phase barrier oracle. Parent reviewed this caveat and does not require additional standalone tests without a concrete uncovered invariant.

### Final stage2 acceptance gate

`cargo xtask check` exited0 on the corrected frozen source. Log: `/tmp/morrow-process-stage2-check-final.log`. Rust result summaries total2465 passed across297 summaries (including one nested child execution; five custom protocol cases are separate),2 ignored. All317 native-output fixtures,20 examples,63 dynamic compatibility programs,295 atomic compatibility rejections,64 grammar cases,192 mutations and231 language-feature mutations passed.

All owned gate processes were reaped; source released to parent for documentation/commit. No commit made by this agent. Parent began a quiet performance diagnostic; new builds/tests remain held until explicit GO.
