+++
schema_version = 1
id = "01M2XHZ8FF00VHCJ8ZSMP7TZGM"
title = "Launch the native checker through a content-validated C bootstrap cache"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for the default quality checker on macOS/Linux

## Decision

I will make the Fern-native checker the default after independent diagnostic/workflow and platform gates, using a Bash 3.2 entry, literal native build arguments and a private content-validated C-bootstrap cache. Python remains an explicit test oracle; ordinary style checks require neither Python nor Cargo.

## Context

The source checker reached exact diagnostics and 66 workflow cases, but a shell job PID could be reaped before cleanup and a stale cached executable could mask changed compiler/runtime inputs. The Bash spike required a small native supervisor retaining child identity. The unavailable `/decision` skill is replaced by this established format; failing cache, supervisor, configuration and recipe tests preceded their implementations.

## Consequences

Immutable snapshots, exact source/tool/dependency contents and bounded lookup inventories govern reuse; permit one freshness retry, never stale fallback. Retained executable identities survive pruning without PID locks. Native supervision preserves normal exits and streams, with bootstrap failure125. Clang14+ uses a private empty explicit config and compiler-scoped default suppression; opaque config/plugin inputs reject. Full checks retain explicit Python integration oracles. Initial helper compilation, escaped process groups and concurrent filesystem freshness have the precise limits in [the launcher contract](../docs/history/NATIVE_STYLE_CHECKER.md). macOS/Linux native matrices, sanitizers, config injection, concurrency and independent workflow gates validate the default switch. This does not switch the language's default compiler from C to Rust.
