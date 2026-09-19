+++
schema_version = 1
id = "01M2XHZ7Z38M2BAZ500YHZFW7P"
title = "Reuse private actor frames and consume sparse allocation maps during collection"
date = "2026-09-19"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted measured checkpoint; no strict parity cell passes and request/reply/lifecycle regressions remain visible

## Decision

Let compiler-private, nonescaping self-tail Unit helper continuations update scalar captures in their existing frame when every owned capture remains bit-identical. Evaluate and root all arguments first; identity or owned-graph changes fall back to ordinary immutable publication without repeating effects. Use the scheduler's validated running descriptor, preserve transient admission before mutation, and keep callback rotation and ordinary closure ABIs unchanged. During GC, count unique marks and consume the allocation map when it has at least 64 blocks and at most one-quarter survive; rebuild only survivor entries instead of rebalancing the old tree for each dead block.

## Safety and independent evidence

Public typed IR cannot forge the private operation. Native tests cover full-width swaps, unchanged and changed owned captures, forced precise collection, evaluation/fault order, cleanup exclusion and ordinary closure behavior. Runtime tests cover quota rollback, malformed/nonrunning calls, exact FIFO rotation and actual owner migration. The allocation oracle changes from 2,560 bytes of frame garbage after 64 callbacks to zero. Sparse-sweep tests retain duplicate/interior roots, cycles, external accounting and address-ordered finalizer/control release; 4,096 dead in-place removals fall to zero while small/dense paths keep their original behavior. Independent review found no blocking safety or test gaps.

## Acceptance

The full macOS ARM64 gate passes 2,393 Rust tests across 292 result suites, 317 native fixtures, 20 examples, 63 dynamic compatibility programs, 295 atomic rejections and 64+192+231 fuzz cases. The ordinary runtime suite has 199 tests. ThreadSanitizer passes 197 in 11.77 s; the two long lifetime-churn tests pass in the ordinary gate. Actor replay preserves trace `0xd4e402a412f11e2f`, 48,739 callbacks and zero cleanup residue.

## Measurements

The [second checkpoint](../benchmarks/language-comparison/results/actors-parity-checkpoint2-20260919/PARITY.md) preserves 36 release smoke checks and 216 unchanged-matrix observations. Default-stealing contention is 4.1–4.9× faster than checkpoint1, reaching only 0.191–0.373× BEAM. Request/reply reaches 0.070–0.262× and lifecycle 0.089–0.568× BEAM; all nine strict cells remain below parity. Two-scheduler lifecycle is slower than checkpoint1. Two separate 144-process diagnostics distinguish [GC-only](../benchmarks/language-comparison/results/actors-parity-sparse-sweep-20260919/STATUS.md) and [frame reuse](../benchmarks/language-comparison/results/actors-parity-frame-reuse-20260919/STATUS.md): GC improves short/long two-scheduler stealing by 1.354×/1.156× but regresses pinned two-scheduler throughput; frame reuse leaves most request/reply cells near their predecessor and the long two-scheduler stealing cell falls to 0.837×. These regressions are retained, not treated as universal improvements. The confirmation gate remains ineligible.
