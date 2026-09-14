# Peer protobuf field registry

`peer.proto` defines the wire format for ALPN `fern.peer.protobuf.v1`.
The peer handshake version is **2**. JSON peer ALPN `fern.peer.v1` is unsupported;
peers reject a mismatched ALPN before parsing application data. The browser's
application version, node configuration, placement digest and checkpoint formats
are independent and remain unchanged.

Messages retain the four-byte unsigned big-endian length prefix and 69,632-byte
frame ceiling. Hello is additionally limited to 256 bytes. A frame contains one
protobuf message; there is no parser fallback or nested JSON. Command and Event
byte fields contain the strict shared `fern-web-protocol::binary` format, capped
at 65,536 bytes before decoding. This byte boundary permits shared bounded codecs
without introducing an additional live-message envelope around a bare Command.

| Message / variant | Required fields | Numeric kind |
| --- | --- | ---: |
| Hello | version=1, cluster=2, node=3, boot=4, link=5, manifest=6 | — |
| Join | kind=1, room=2, lease_ms=3 | 1 |
| Command | kind=1, command=4 | 2 |
| Event | kind=1, event=5 | 3 |
| Ping | kind=1, nonce=6 | 4 |
| Pong | kind=1, nonce=6 | 5 |
| Close | kind=1 | 6 |

All fields are singular. Every required field must be present even when its value
is zero. No other fields are permitted for a selected variant. Unknown fields,
duplicates, unknown kinds, groups, wrong wire types, overflowing or overlong
varints, invalid UTF-8 and truncated lengths are rejected. Record order is not
significant. The encoder writes ascending field numbers. This strict profile
does not promise that arbitrary protobuf tooling's defaults satisfy Fern's
presence/unknown-field rules, nor that encoded bytes define message identity.

The shortest-varint requirement applies to the **outer peer Hello and Frame**,
including field keys and length prefixes inside those protobuf messages. Nested
Command/Event bytes use the shared live protobuf decoder, which accepts bounded
nonminimal varints permitted by protobuf. Both layers reject overflow, duplicate
singular fields and unknown fields. They compare decoded logical identities,
not encoded bytes. The independent vectors in `tests/binary.rs` explicitly reject
overlong outer kind and nonce encodings.

Boot and link are exactly 16 bytes and cannot be all zero. Manifest is exactly
32 bytes. Cluster and node use their existing validated ASCII identities. Room
uses the existing 128-byte/no-controls rule; lease remains 1–3,600,000 milliseconds.
Peer TLS authentication still binds the claimed node to its configured leaf
certificate and validates exact cluster and manifest agreement.

The parser borrows length-delimited fields while validating the outer envelope.
It allocates owned application data only through the bounded shared binary
decoder. It does not deserialize native pointers or bypass Hub/domain validation.
Cancellation-safe partial reads, poisoned partial writes, absolute deadlines,
authority checks, admission budgets and unknown-completion semantics are
independent of the encoding.

These field numbers, variant numbers and wire types are permanent assignments.
Never recycle removed numbers. The initial profile is closed: adding a field or
semantic behavior requires an explicitly negotiated supported protocol revision.
Dynamic rolling upgrades are not implied by using protobuf.

Independent bytes in `tests/binary.rs` and `tests/framing.rs` pin the registry.
Real TLS tests reject missing certificates, forged node identities and old ALPNs,
and exchange 10,000 full-width integer/Unicode commands. Large hostile raw-frame
tests preserve allocation/deadline coverage independently of the stricter
application-level string and collection limits.
