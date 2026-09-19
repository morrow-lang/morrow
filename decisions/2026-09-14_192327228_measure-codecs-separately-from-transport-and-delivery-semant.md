+++
schema_version = 1
id = "01M2XHZ83WM5CAKWAMGPJ45HKQ"
title = "Measure codecs separately from transport and delivery semantics"
date = "2026-09-14"
status = "deprecated"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2XHZ83PY052N3N96FTQQ48R"]
+++
## Status

Historical baseline; JSON retention superseded by [Decision 145](2026-09-14_192327222_standardize-live-browser-and-peer-messages-on-a-bounded-prot.md) after browser and message-path measurements

## Decision

Keep bounded JSON over WebSocket for browsers and bounded JSON frames over TLS for peers. Compare native-i64 CBOR and protobuf adapters in an excluded benchmark workspace, preserving real message identities and required fields. Do not introduce gRPC-Web as the bidirectional browser channel.

## Context

The current protocol already preserves full-width integers with decimal strings, bounds external records and distinguishes command identity from socket delivery. A smaller encoding does not supply backpressure, reconnection, idempotency or durable acknowledgement. Official gRPC-Web still lacks client/bidirectional streaming; protobuf itself is independent of gRPC.

## Consequences at this decision

Forty-six actual message fixtures and explicit malformed-input tests compared bytes and native encode/decode costs. Browser bundle size, allocation and application-path performance were then unmeasured; benchmark codecs stayed outside production. [Decision 145](2026-09-14_192327222_standardize-live-browser-and-peer-messages-on-a-bounded-prot.md) records the subsequent browser and compiled-application evidence. The requirement to distinguish codec, transport and delivery effects remains in force.
