+++
schema_version = 1
id = "01M2XHZ8YRSHCVAW6835CTFZMJ"
title = "Constrained Step C dup/drop insertion in codegen"
date = "2026-02-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will implement initial dup/drop insertion in codegen only for a constrained, test-covered subset: pointer alias let-bindings (`let y = x`) emit `fern_dup`, and function-scope owned pointer names emit `fern_drop` at return sites with returned-identifier preservation.

## Context

Milestone 7.7 Step C requires proving end-to-end codegen insertion before full ownership analysis. A broad first pass (all expressions/scopes/branches) would be high-risk and hard to validate in one step.

## Consequences

Fern now emits semantic `dup/drop` calls for simple pointer ownership flows while keeping behavior deterministic under the current Boehm bridge. Coverage is anchored by focused codegen regression tests in `tests/test_codegen.c` (`test_codegen_dup_inserted_for_pointer_alias_binding`, `test_codegen_drop_inserted_for_unreturned_pointer_bindings`), and broader ownership inference remains future work.
