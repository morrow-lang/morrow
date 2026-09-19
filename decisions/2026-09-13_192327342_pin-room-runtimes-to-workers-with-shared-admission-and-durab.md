+++
schema_version = 1
id = "01M2XHZ87EDJ3YMFKYEKJ3VANA"
title = "Pin room runtimes to workers with shared admission and durable ownership"
date = "2026-09-13"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; deterministic worker and real WebSocket tests pass

## Decision

Route rooms by a stable hash to independently owned native runtimes on 1–32 pinned OS threads. Default to available parallelism capped at four. Keep authentication in a separate owner and pass revocable, expiring capabilities to workers. Share one ingress semaphore and RAII room, namespace and connection quotas across all workers. Share durable checkpoints through a Rust-owned locked writer, comparing expected state before each commit.

## Context

A single owner serialized all room execution and delayed logout behind application work. Sharing native pointers between threads would violate the collector contract. Independent runtimes allow unrelated room progress while preserving thread ownership; global leases prevent worker count from multiplying process limits. Concurrent checkpoint handles must preserve every room and reject stale owners that could overwrite acknowledged state.

## Consequences

Commands within a room retain their owner order. Tests hold one domain callback at a deterministic gate and require another worker and authentication to progress. Logout rejects queued work; an already executing command may finish. Cross-worker reconnect waits for old connection removal, including at capacity. Durable writes serialize at the shared writer and may delay other commits. This establishes bounded room sharding, not general actor preemption, work stealing, live migration, replicated ownership or Erlang-style clustering.
