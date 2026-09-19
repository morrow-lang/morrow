+++
schema_version = 1
id = "01M2XHZ8EARSCM6VKBK0JF7QN0"
title = "Stop supervised descendants before fallible exit notifications"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for deterministic mailbox lifecycle; actor execution remains open

## Decision

I will stop the complete owned descendant subtree before any exit notification can fail. Descendants receive shutdown semantics, and surviving external observers receive root-first notifications in child registration order. A child cannot restart under a dead owner.

## Context

Supervisor exits left descendants alive and schedulable, including after notification allocation failures. Recursive exit calls would introduce stack limits and restart children during parent shutdown. Additional allocation tests reproduced spawn accepting a failed name copy and restart publishing a live orphan before monitor storage failed.

## Consequences

A bounded allocation-free iterative ownership walk clears lifecycle, current-context and scheduler state before notifications. The first notification failure is returned; later notifications and automatic restarts are not promised after failure. Normal/shutdown exits do not automatically restart. Existing abnormal direct-owner strategies may replace the root, but do not rebuild its former child subtree or reparent old children. Prepare actor names and replacement monitor storage before ID publication; the existing fatal heap-Result wrapper allocation policy is unchanged. Ten subtree/failure groups and six prior lifecycle scenarios pass debug/release/sanitizers on macOS/Linux arm64. Actor execution, ancestor escalation, subtree reconstruction and the million-step FernSim target remain separate. The unavailable `/decision` skill is replaced by this established format.
