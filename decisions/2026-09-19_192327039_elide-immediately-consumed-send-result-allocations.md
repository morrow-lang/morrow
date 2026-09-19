+++
schema_version = 1
id = "01M2XHZ7XZG1B42V2EGTRM16R9"
title = "Elide immediately consumed send Result allocations"
date = "2026-09-19"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; all twelve request/reply diagnostic medians improve

## Decision

Lower eligible direct `match send(...)` expressions through a private scalar outcome while sharing the runtime validation, copying and admission path with the public boxed send. Zero means Ok(Unit); existing nonzero errors retain their Int payload. Ordinary stored/returned Results, whole-Result bindings and unsupported patterns retain the normal RC-prefixed representation. Match validation, work/depth charges, source Result duties, operand/guard order, fault checks and roots remain unchanged.

## Evidence

Independent red/green tests count128→0 runtime Result allocations and1→0 actual compiled-source allocations. Native tests cover full-width Int/Float/String/Pid delivery, actual callbacks, ordered guards, precise GC, fault/defer behavior and boxed alias fallback at S1/S2/S4 with stealing off/on. The full gate passes2447 Rust tests across298 summaries, every native/example/compatibility/fuzz check; TSan232 and deterministic replay pass, as do36 release smoke checks.

## Measurement

The [quiet144-process comparison](../benchmarks/language-comparison/results/actors-parity-unboxed-send-20260919/STATUS.md) improves all twelve median request/reply cells by1.042–1.292× against the corrected cafc91d2 baseline. Preserve all raw streams, paired-round values and frozen hashes. This comparison does not establish BEAM parity; the last completed full matrix remains zero of nine strict cells.

## Limits

No allocation claim applies to arbitrary escaping Results or unsupported match shapes. Future error0 would require revising the private scalar convention. Physical allocation and automatic collection timing intentionally change; logical message admission and the public boxed ABI do not.
