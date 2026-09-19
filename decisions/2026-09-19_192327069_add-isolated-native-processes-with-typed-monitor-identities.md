+++
schema_version = 1
id = "01M2XHZ7YXSWSBJYGZJV0V4XG9"
title = "Add isolated native processes with typed monitor identities and ordered lifecycle events"
date = "2026-09-19"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2XHZ7Z92KTQQ4DSF99CPG43", "01M2XHZ7Z38M2BAZ500YHZFW7P", "01M2XHZ7YQCS140KS27WVVZJWT"]
+++
## Status

Adopted monitor foundation; links and supervisor policies remain open

## Decision

Add `Process.spawn`, atomic `spawn_monitor`, `self`, mailbox-erasing `id`, independent `monitor`/`demonitor` and `receive_event`. Select an immutable isolated failure policy at child publication; keep ordinary `spawn`, existing lifetime supervision, root faults and infrastructure failures unchanged. Carry `Message(M)` and system `Down` cells through the same ingress and ordered mailbox, with ordinary receive skipping system cells. Use distinct opaque descriptors 13/14 for ProcessId/MonitorRef and retained invocation epochs and generations for allocation-free identity equality. The public Exec/Type/Function/Pid ABI layouts remain unchanged.

## Ownership and admission

Reserve each monitor completion before success, with 256 per observer, 4,096 per invocation and 512 logical bytes each inside the existing 64 MiB bound. An owner-local ledger carries the reservation through active, pending and queued states until cancellation, consumption or retirement; a full user mailbox cannot discard an admitted Down. Conservative physical metadata accounting is separate. Shared indexes hold weak controls; wrappers retain immutable identities and epochs without keeping another actor's heap alive. Registration and death serialize under target-route then registry locks, including when the registry is first created during legacy retirement. Publication, generated code and GC allocation occur outside those locks. Migration retains system cells and accounting without pinning otherwise movable processes.

## Liveness and cancellation

A live isolated process permits indefinite idle waiting without the legacy deadlock fault. An owned cross-thread host cancellation token wakes parked execution without sharing a mutable Exec. Registry identity survives configuring multiple schedulers after token creation. Local and parallel cancellation preserve prior and cleanup faults. Valid generated callback statuses with an existing checked fault follow isolated retirement; unsupported status values and infrastructure failures still stop the invocation while preserving the first diagnostic cause.

## Independent evidence

Native fixtures check exact full-width payloads, copied identity/reference equality, distinct monitors, message-before-Down ordering, sibling completion, self/dead monitor cases, option semantics and both receive views. Both fixtures pass at one/two/four schedulers with stealing off/on, with precise collection on the actual callback owner. Deterministic runtime regressions cover quota boundaries/churn, forced construction GC, migration, registration/retirement races, pending cancellation, final-stop publication and liveness/cancellation barriers. The multi-scheduler native matrix exposed a real callback-fault regression, retained as a red-to-green ABI oracle. [Pinned OTP fixtures](../docs/process-model/README.md) independently establish matching monitor edge cases and future link/supervisor expectations.

## Acceptance

`cargo xtask check` passes 2,415 Rust tests across 295 result suites, 317 native-output fixtures, 20 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192+231 fuzz cases. ThreadSanitizer passes 212 runtime tests in 28.40 s without race reports; the two long churn cases pass in the ordinary 214-test runtime suite. Default actor replay retains trace `0xd4e402a412f11e2f`, 48,739 callbacks and zero cleanup residue. An external `deny(improper_ctypes)` ABI probe preserves the web host boundary: lifecycle metadata is opaque through the existing C-compatible Exec layout.

## Integrated acceptance

After combining this foundation with Decisions [163](2026-09-19_192327081_select-heap-transfer-validation-by-the-live-graph-and-foreig.md)/[164](2026-09-19_192327075_reuse-private-actor-frames-and-consume-sparse-allocation-map.md)/[166](2026-09-19_192327063_cache-immutable-actor-descriptors-with-unchanged-work-accoun.md), the full gate passes 2,432 Rust tests across 295 suites and the same 317 native fixtures, 20 examples, compatibility and fuzz cases. ThreadSanitizer passes 224 runtime tests in 15.78 s; the two long churn tests pass in the ordinary 226-test suite. Replay remains exact. Integration preserves both private-frame reuse and Process lowering, with the external Exec ABI check passing against the descriptor cache as well.

## Admission compatibility correction

Expanded lifecycle metadata must not raise ordinary-message admission. Preserve the historical 32-byte logical message header while physical accounting follows the actual allocation; scalar payloads add 8 bytes. An independent literal-boundary regression fails before the correction and passes afterward: 39 spare bytes reject without publication, 40 accept the full-width scalar, with exact local and shared budget rollback. Existing String rejection and copied JSON ownership/cost oracles retain their original logical policy. The corrected tree passes the full gate (2,433 Rust tests across 295 suites and all native/example/compatibility/fuzz checks), the new boundary under ThreadSanitizer, and unchanged actor replay.

## Boundaries

This completes only the isolated-process/monitor foundation. Link effects, exit signalling, restart strategies, rolling intensity, ordered/nested shutdown and dynamic significant-child policies remain subsequent stages. The Exit event and complete typed reason schemas are declared now, but this runtime stage only emits Down with Normal, Fault or NoProcess. Native code remains cooperative; host ports, remote identities and REPL process execution are unsupported. [Implemented API and limits](../docs/PROCESS_MODEL.md); [remaining local OTP plan](../docs/superpowers/specs/2026-09-19-typed-otp-process-model.md). No actor-performance or general OTP parity claim follows from this API work.
