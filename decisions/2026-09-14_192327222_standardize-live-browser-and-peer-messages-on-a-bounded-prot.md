+++
schema_version = 1
id = "01M2XHZ83PY052N3N96FTQQ48R"
title = "Standardize live browser and peer messages on a bounded protobuf profile"
date = "2026-09-14"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; measured prototypes, integrated macOS repository gate and real-browser migration acceptance pass
* **Decision**: Use `fern.live.protobuf.v1` binary WebSocket for the first-party browser and `fern.peer.protobuf.v1` ALPN with peer handshake version 2 between servers. Retain an explicitly negotiated `fern.live.v1` JSON browser compatibility path. Keep HTTP/admin, node configuration, offline records, checkpoints and the native domain bridge in their independently versioned JSON formats. Protobuf does not require gRPC.
* **Context**: The real-browser experiment measures 100-task ASCII snapshot decode including transfer at 26.803/11.071/7.301 µs for JSON/CBOR/protobuf. Protobuf has the smallest complete experiment module; CBOR can encode slightly faster and use fewer bytes. In 7,650 mutations through the real Hub and compiled Fern actor, 100-task round trips remain approximately 2.05 ms ephemeral and 12.9 ms durable across codecs, while protobuf reduces the combined payload by approximately 64%. Measured codec wall-time fractions are not CPU-utilization shares, and the native-domain interval includes actor execution and its JSON bridge rather than isolating either.
* **Consequences**: Explicit field registries, required presence, exact sint64 values and preallocation limits form a shared closed profile. Unknown/duplicate/irrelevant fields reject; generic protobuf decoder defaults alone are insufficient. Stable numbers do not imply rolling upgrades: new schema semantics require explicit negotiation, old peer ALPNs fail, and peers require a coordinated upgrade. Typed command identity, bounded queues, authorization, uncertainty and checkpoint placement remain independent of encoding. Production promotion passed malformed-input simulations, cross-codec local/remote clients, real-browser offline/reconnect and asset integrity, TLS/fault/stress and the complete macOS repository gate. Exact acceptance scope is recorded in the roadmap. See `protocol/README.md`, `docs/WIRE_STANDARD_PROPOSAL.md` and `docs/NETWORK_PROTOCOL.md`.
