+++
schema_version = 1
id = "01M2XHZ7XR5SNV8VMNK49RV5QH"
title = "Account for removed self-calls in bounded actor tail batches"
date = "2026-09-19"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted measured compiler improvement; strict parity remains zero of nine
* **Decision**: Count expanded whole-tree work as 1+copies*(cost-1), because every eligible expansion replaces exactly one self-call charged 1. Use `(STEP_WORK-1)/(cost-1)` with the existing eight-copy bound for cost 1. Keep the original-work <=16 guard, STEP_WORK 32, node/depth caps, effect boundaries and ordinary native function ABI. The frozen hot loop executes three iterations at 31 work units instead of two at 21; no callback quantum changes.
* **Independent evidence**: Preserve red/green work 31 and actual three-update callback oracles. A3000-step native recurrence completes within 1100 polls with exact checksum 315511746, a sibling within 10 polls and precise collection. Argument/fault/defer/full-width and ordinary ABI regressions pass. The full gate reports 2451 Rust passes across 299 summaries and all native/example/compatibility/fuzz checks; 36 release smoke checks pass. Runtime archive equality preserves the accepted 232-test TSan and exact actor replay evidence.
* **Measurement**: The [72-process paired contention diagnostic](../benchmarks/language-comparison/results/actors-parity-tail-batch-exact-20260919/STATUS.md) improves all six medians 1.439–1.460× over unboxed-send. The [216-process fourth matrix](../benchmarks/language-comparison/results/actors-parity-checkpoint4-20260919/PARITY.md) reaches contention ratios 0.785/0.373/0.398, but still passes zero of nine strict or competitive cells. Preserve the 18.8% S4 request/reply regression versus checkpoint3 and all raw evidence. Confirmation remains ineligible; no overall BEAM or OTP parity claim follows.
