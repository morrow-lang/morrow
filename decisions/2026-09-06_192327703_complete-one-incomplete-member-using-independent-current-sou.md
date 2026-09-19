+++
schema_version = 1
id = "01M2XHZ8JQ4HC28HHG80PTK1YD"
title = "Complete one incomplete member using independent current-source evidence"
date = "2026-09-06"
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

I will recover a single member selector for completion only when its receiver is already concrete and the enclosing function group has an independently fixed concrete signature. Recovery is an explicit partial proof with non-executable IR, never repaired source or a guessed member.

## Context

Ordinary checked source facts cannot cover the moment a user types `value.`. Feeding missing-operation constraints into whole-signature inference could fabricate receiver types, while candidate replacement can hide independent errors. The parser can preserve source offsets using one private token and an opaque site.

## Consequences

The ordinary graph, signatures, unaffected bodies and local constraints still validate. Only local hole-dependent unknowns may remain private editor markers; they never justify receiver evidence or enter schemes/QBE/REPL. Current overlays and exact UTF-16 edits are preserved, all existing limits apply, and unrelated errors retain lexical fallback. Editor library checking uses the real source graph without inserting main. See [the recovery contract](../docs/history/EDITOR_RECOVERY.md). The unavailable `/decision` skill is replaced by the established decision format.
