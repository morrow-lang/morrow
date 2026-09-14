# Fern network protocol and transport evaluation

Date: 2026-09-14. Status: current browser decision and implemented fixed-owner peer transport. An isolated native
codec benchmark is implemented; browser and network comparisons remain proposals.

## Current decision and implementation

Keep WebSocket with bounded JSON as the browser default. Separate the message
contract, serialization format and transport so each can evolve without changing
application delivery semantics. A binary encoding is not evidence of a faster
application, and protobuf does not require gRPC.

The current implementation is `crates/fern-web-protocol`, particularly `src/wire.rs`,
`src/hub.rs`, `src/client.rs` and `tests/traces.rs`. It is a shared Rust protocol for
the collaborative checklist preview, not a generated codec for arbitrary Fern
types or a distributed actor protocol. Its current properties include:

- Version 1 envelopes, a 65,536-byte input/output cap and a capped output writer.
- Canonical decimal-string `i64` values; numeric JSON values, leading zeroes,
  negative zero and out-of-range integer strings are rejected for these fields.
- Closed schemas with duplicate/unknown struct fields rejected.
- Separate resource incarnation, resumable command namespace and connection
  identity; sequences, revision conflicts and bounded retained outcomes.
- A sequence high-water mark that prevents execution of an old command after its
  cached outcome expires. Expiry produces an unknown outcome, not a replay.
- Explicit authorization, reset, resynchronization and uncertain-completion paths.

`docs/WEB_PREVIEW.md` describes the running browser host and server. Read
`docs/FULL_STACK_ARCHITECTURE.md` for the broader direction. Neither transport
choice nor this document establishes general remote-PID routing or replicated
durability. Fixed room-owner forwarding and local durable checkpoints are
implemented; see [cluster operation](CLUSTER.md).

Ordinary JSON numbers are not a portable full-width integer representation:
RFC 8259 identifies the interoperable integer range as ±(2^53−1). The existing
string convention should remain for JSON. ProtoJSON similarly emits 64-bit
integers as strings. Binary decoders should deliver integers directly into
Rust/WASM `i64`, never through JavaScript `Number`.
[JSON numeric interoperability](https://www.rfc-editor.org/rfc/rfc8259.html#section-6),
[ProtoJSON format](https://protobuf.dev/programming-guides/json/).

## Candidate encodings

| Encoding | Assessment and current evidence |
| --- | --- |
| Current JSON | Keep the implemented baseline, readable diagnostics and exact decimal-string integers. Measure its actual encoder/decoder rather than an unrelated JSON library. |
| CBOR | A useful first binary candidate with native integer and byte-string types. Propose a small definite-length profile, stable numeric field tags, explicit sum tags, finite floats, bounded containers and rejection of duplicate keys. The isolated benchmark shows smaller messages for tested fixtures, without establishing a universal winner. |
| Protobuf | A strong comparator when generated schemas and other-language clients matter. Choose stable field numbers, preserve required-field presence in adapters and reserve removed numbers. Native protobuf can be carried by the existing WebSocket; no gRPC service is needed for this experiment. |
| MessagePack | Also has integer and binary types and merits a later comparator. It does not itself define Fern schema evolution, bounded allocation or delivery guarantees. Start with three encodings to keep the first experiment small. |

CBOR and MessagePack both encode 64-bit integers. CBOR additionally specifies
preferred/deterministic serialization considerations; a Fern profile must still
define the accepted subset and limits. Neither a general-purpose decoder nor a
small input alone guarantees appropriately bounded allocation for declared lengths.
[CBOR specification](https://www.rfc-editor.org/rfc/rfc8949.html),
[MessagePack specification](https://github.com/msgpack/msgpack/blob/master/spec.md).

Protobuf's numeric field identities support compatible changes when its rules are
followed. Do not recycle tags or equate successful decoding with semantic
compatibility. Test unknown fields against the actual chosen Rust implementation,
especially if messages are decoded and forwarded. The current JSON
`deny_unknown_fields` policy means adding fields is not automatically compatible
with old clients either. Negotiate a supported schema/version before application
messages and retain explicit upgrade errors.
[Protobuf language guide](https://protobuf.dev/programming-guides/proto3/),
[Protobuf evolution guidance](https://protobuf.dev/best-practices/dos-donts/).

## Browser and server transports

WebSocket carries both text and binary messages, so changing encoding need not
change deployment. Preserve bounded queues and browser `bufferedAmount` checks;
the classic browser WebSocket API does not expose receive backpressure. Application
credits, coalescing replaceable snapshots and disconnect/resync policies still
matter. Never coalesce acknowledgements as though they were snapshots.
[WebSocket standard](https://websockets.spec.whatwg.org/),
[WebSocket API](https://developer.mozilla.org/en-US/docs/Web/API/WebSockets_API).

gRPC-Web currently supports unary and server streaming, not client or
bidirectional streaming. The official client's text-streaming mode uses base64.
This is a poor fit for replacing Fern's continuous bidirectional UI connection.
Rust `tonic-web` can embed translation in the server, so a separate Envoy process
is not inherently mandatory; it does not remove the streaming limitation.
[Official gRPC-Web](https://github.com/grpc/grpc-web),
[tonic-web documentation](https://docs.rs/tonic-web/latest/tonic_web/).

WebTransport is now a real optional candidate. Safari 26.4 added it in March 2026,
and MDN marks it newly Baseline since that month. Do not describe it as generally
unavailable in Safari. It offers reliable streams, datagrams and stream
backpressure; independent streams can avoid blocking each other on loss. Older
clients, server support and actual network/intermediary behavior still require
testing and fallback. Use reliable streams for mutations; reserve datagrams for
replaceable cursor/presence state. Adoption depends on measurements, not API novelty.
[WebKit announcement](https://webkit.org/blog/17862/webkit-features-for-safari-26-4/),
[current compatibility](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport),
[WebTransport specification](https://w3c.github.io/webtransport/).

Phoenix's current default serializer encodes ordinary messages as a JSON array
containing join reference, request reference, topic, event and payload, with a
separate binary path for `ArrayBuffer`. LiveView uses Phoenix Socket. The useful
reference is compact framing and distinct message identities; Fern's browser-owned
model/update/view remains a different rendering architecture.
[Phoenix serializer source](https://raw.githubusercontent.com/phoenixframework/phoenix/main/assets/js/phoenix/serializer.js),
[LiveView socket source](https://raw.githubusercontent.com/phoenixframework/phoenix_live_view/main/assets/js/phoenix_live_view/live_socket.ts).

## Server link contract

The [implemented cluster](CLUSTER.md) uses bounded length-prefixed JSON over
mutually authenticated TLS/TCP. A certificate-bound handshake carries protocol,
node, boot, stream and manifest identities. Each browser subscription owns one
stream; existing typed commands carry resource incarnation, namespace and
sequence. Admission, fixed-owner authorization, bounded queues and deadlines are
explicit. Native heap addresses, closures and language PIDs stay local.

Erlang distribution provides a useful behavioral reference: connection-oriented
transport, a handshake and length framing, node identities and monitoring. Its
packet prefix changes from two bytes during the handshake to four afterward.
Its default cookie mechanism is not cryptographic protection and default traffic
is cleartext; Fern should authenticate and encrypt nonlocal links.
[Distribution protocol](https://www.erlang.org/doc/apps/erts/erl_dist_protocol.html),
[distributed Erlang security](https://www.erlang.org/doc/system/distributed.html).

Separate local queue acceptance, destination mailbox admission and application
commit acknowledgements. A lost link can leave completion uncertain. Automatic
retry requires idempotency state committed with the effect; ordinary actor
supervision is not durable exactly-once execution. Erlang also permits loss when
a distribution channel fails and only guarantees signal ordering for a particular
sender/destination pair.
[Erlang signal semantics](https://www.erlang.org/doc/system/ref_man_processes.html).

Fixed multi-node room placement and forwarding are implemented and stress-tested.
Transparent remote PIDs, dynamic membership, distributed transactions and
replicated failover remain proposals. A replacement forwarding stream creates a fresh
namespace; the browser reports uncertain completion instead of replaying mutations.

## Isolated codec measurements and further experiments

The standalone unpublished crate at `benchmarks/network-codecs` now has its own
`[workspace]`, exact dependency pins and lockfile, following the existing
`benchmarks/compiler-phases` pattern. It is excluded from the root workspace. A
path dependency on `fern-web-protocol` supplies the real message types; binary
codec dependencies remain outside the production workspace and artifacts.

The benchmark uses `minicbor` and `prost`; both document `no_std` support. That is
feasibility evidence, not a bundle-size result. Its Rust harness uses
`std::time::Instant` and `std::hint::black_box` without a statistics or async runtime
dependency. Benchmark build artifacts should use a separately owned target
directory with an explicit disk budget.
[minicbor](https://docs.rs/minicbor/latest/minicbor/),
[prost](https://docs.rs/prost/latest/prost/).

The benchmark has two layers:

1. Codec-only measurements on prepared representations, clearly excluding adapter
   cost. For JSON, call the current `fern_web_protocol::encode` and `decode`.
2. End-to-end conversion from and back to the actual `ClientMessage` and
   `ServerMessage` types, including allocations and field validation performed by
   binary adapters. This is the relevant comparison for changing production code.

Do not simply serialize `Decimal` through a generic binary Serde encoder: its
current serializer emits text, which would miss the proposed binary integer
representation. CBOR/protobuf adapters must map `Decimal.0` to a native signed
integer and reconstruct it exactly. Record protobuf `sint64` versus `int64`
selection explicitly. Use optional scalar presence where necessary to detect
missing required fields; a default false/zero must not silently stand in for a
missing field that current JSON rejects. Generated derive-based protobuf structs
can avoid a build-time `protoc` dependency for this isolated experiment; retain a
readable field-number schema. Test independent expected bytes for representative
cases, not just adapter-to-itself round trips.

Build deterministic fixtures covering every client/server envelope and mutation,
all outcome statuses, join/resume/reset, snapshots with 0/1/10/100 tasks, ordinary
ASCII and multibyte labels, maximum valid labels and escape-heavy text. Include
`i64::MIN`, `i64::MAX`, ±2^53 boundaries and zero in codec-only fixtures; separately
identify which values domain validation permits. Never label a deliberately
invalid negative sequence as a valid application command. Reuse actual Hub trace
outcomes as behavioral fixtures and keep a fixed seed and corpus digest.

Malformed tests cover truncation, duplicates, unknown fields/tags, missing fields,
overflow, invalid UTF-8, oversized frames, excessive nesting and huge claimed
container lengths. Require bounded rejection and unchanged application state.
The prototype must report differences in acceptance policy rather than silently
calling them equivalent. Migration requires resolving those differences first.

Outside timed loops, prebuild inputs, verify decoded equality and record exact
wire bytes. Warm up, collect repeated bounded batches, rotate codec order with a
fixed seed and consume decoded results through `black_box`. Report batch medians
and dispersion; do not call batch percentiles per-message p99 latency. Measure
allocation counts separately so instrumentation does not contaminate throughput.
Record compiler version, target, features, dependency lock hash, source revision,
dirty-state manifest, optimization flags, hardware, OS, corpus seed, iterations,
input/output byte counts and timing units in machine-readable results. Preserve raw
samples. Compare compression only as an explicitly separate experiment.

First run native measurements; then compile equivalent adapters for the real
browser host and measure WASM plus required glue, raw and Brotli bytes, cold load,
steady decode time and memory in current Chromium, Firefox and Safari. A native
microbenchmark cannot establish browser performance or network throughput.

Only after codec measurements, hold the selected encoding constant and compare
WebSocket, TLS/TCP node links and optional WebTransport. Stress 2/3/8 nodes with
increasing senders, fan-out, a hot receiver and slow consumers. Measure acknowledged
throughput, per-command latency distributions, fairness, queue rejection and
memory. Inject partitions, delayed/lost acknowledgements, reconnect storms,
duplicate requests, stale generations and mixed versions under deterministic
seeds. Assert no duplicate committed mutation, no cross-incarnation delivery,
bounded backlogs and explicit uncertainty.

The [isolated benchmark](../benchmarks/network-codecs/README.md) now records
46 native message fixtures and six correctness groups. On the recorded Apple M4
run, an Add command used 202/69/66 bytes for JSON/CBOR/protobuf, and a 100-task ASCII
snapshot used 6,379/2,548/2,745 bytes. The benchmark includes raw timing samples,
adapter/validation measurements, dependency pins and hardware/source fingerprints.
Concurrent project work limits interpretation of its timing observations. It does
not measure WASM/glue size, browser decoding, network throughput or end-to-end UI.

Retaining current WebSocket/JSON remains the decision. Binary adapters are confined
to the excluded benchmark package; they are not enabled in production. The test
corpus above remains the broader migration target, including stateful Hub traces
and faulted network experiments beyond the implemented codec microbenchmark.
