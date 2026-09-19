+++
schema_version = 1
id = "01M2XHZ8D6NZXNG8AY8FG1JTFF"
title = "Respect Apple arm64 reserved registers in the shared backend"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for the Apple arm64 QBE target

## Decision

I will reserve IP1/x17 in the Apple allocator and use it for every integer swap/spill scratch path. Keep x18/w18 unavailable, as required by [Apple's ABI](https://developer.apple.com/documentation/xcode/writing-arm64-code-for-apple-platforms). Generic arm64/Linux keeps its original allocation and scratch convention.

## Context

A stalled native checker had crashed while dereferencing a tuple; its generated return path used reserved x18. An assembly invariant and controlled scalar clobber test independently reproduced the ABI defect before the fix. The sample does not establish the exact crash trigger, and no separate GC or tuple-lifetime defect was identified. Fern imported this vendored QBE source in commit `217960c40ab7e434750e7111f4c8d3277933db3b`; that import records no upstream release/tag. The unavailable `/decision` skill is replaced by this established format.

## Consequences

Allocator global masks/counts and all integer scratch emission agree; caller-save filtering preserves the dedicated scratch and Float scratch remains v31. Apple assembly contains no x18/w18 for swaps, forty live values, calls and constant spills. Independent native outputs pass on macOS/Linux; generic Linux assembly is byte-identical. Twenty fresh native checker scans produce identical output. This is stability evidence, not proof of the original crash cause. C and Rust share the corrected backend; launcher, GC, recursion and timeout behavior are unchanged.
