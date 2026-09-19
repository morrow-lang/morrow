+++
schema_version = 1
id = "01M2XHZ8N5PWJK7XZ5GHRYQJVY"
title = "Generalize private signatures by recursive component"
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

I will infer omitted private parameter and return types from patterns and function bodies in callee-first recursive components, then publish closed schemes with intrinsic capability requirements. Calls to completed schemes instantiate fresh variables; unfinished inferred recursive members share monotypes.

## Context

Pattern-only inference requires annotations for ordinary identity and higher-order helpers. Using whichever caller is visited first would make types order-dependent. Explicit generic annotations remain universal and must not be weakened to make inference succeed; existing complete-parameter return inference already handles annotated mutual recursion with distinct generic names.

## Consequences

Public boundaries remain annotated and omitted main remains Unit. Explicit type variables stay rigid, local bindings stay monomorphic, and inferred polymorphic recursion requires a complete annotation. Generalization preserves parameter/result relationships and rejects unanchored recursive results or ambiguous requirements. Source-provided Infer remains forbidden. Dependency traversal, constraints, recursive solving and scheme closure share bounded work. Core generalization lands before explicit delayed shape obligations; the milestone stays open until later body evidence can resolve fields, updates, iteration and tuple-rest shapes without guessed types or fake values. The unavailable `/decision` skill is replaced by the established decision format.
