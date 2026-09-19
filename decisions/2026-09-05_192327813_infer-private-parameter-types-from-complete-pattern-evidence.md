+++
schema_version = 1
id = "01M2XHZ8P5YBJYX023WG9JQX0S"
title = "Infer private parameter types from complete pattern evidence"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for Rust migration completion

## Decision

I will collect constraints from every clause pattern and supplied parameter annotation before normalizing a private function group. This stage fills omitted annotations only when their types are fully determined, including explicitly anchored generic variables.

## Context

Literal and constructor patterns often establish a function's input type without any caller. Using the first caller as evidence would make otherwise generic functions depend on call order. Full private signature generalization needs a separate recursive-component solver.

## Consequences

All clauses constrain one slot per parameter position. Public parameter annotations remain mandatory. Empty lists, generic nullary constructors, catchalls and tuple-rest shapes alone may remain ambiguous and require annotations until whole-signature inference lands. Constructor schemas use fresh variables, explicit generic names remain rigid, conflicts report source diagnostics, and pattern/type work has one bounded budget across the pass. Existing coverage and Result checks still run after normalization. The unavailable `/decision` skill is replaced by the established decision format.
