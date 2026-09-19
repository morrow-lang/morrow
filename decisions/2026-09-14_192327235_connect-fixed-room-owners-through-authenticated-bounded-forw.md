+++
schema_version = 1
id = "01M2XHZ843GP176BHPGQH5CYR0"
title = "Connect fixed room owners through authenticated, bounded forwarding streams"
date = "2026-09-14"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; core simulations, real TLS and three-process fault/stress tests pass

## Decision

Configure 1–16 nodes, choose room ownership with versioned rendezvous hashing, and authenticate each browser subscription's peer stream with mutual TLS plus certificate-bound node/boot/manifest identities. Keep ordinary standalone operation and browser WebSocket v1. Bind local checkpoint directories to immutable cluster/node/placement identities while separating certificate/address rotation from placement.

## Context

A gateway should reach another server's native room actor without exchanging heap pointers or requiring a broker. A partition cannot safely grant a new owner authority over the same data. Retrying an uncertain mutation into a new process namespace could duplicate a committed effect.

## Consequences

Persistent ordered streams carry typed owned commands, bounded outcomes and coalesced snapshots. Limits cover frames, streams, handshakes, ingress and pending commands. Owner-observed transport teardown or lease expiry invalidates delegated capabilities before queued native work executes; a partition can delay observation of gateway logout, and already admitted work may finish. Every replacement stream has a fresh namespace, with explicit browser uncertainty and no automatic mutation replay. Local checkpoint recovery is preserved; dynamic membership, replicated failover, remote language PIDs and distributed transactions remain separate capabilities. The independent stress test drives 10,024 durable mutations through two gateways to a third owner, then exercises balanced owners, slow readers, partitions and restart. See `docs/CLUSTER.md`.
