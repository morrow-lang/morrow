# Fern network protocol: decision and measurements

Date: 2026-09-14. Decision: adopt a bounded, explicitly negotiated **protobuf**
profile for live browser/WASM/server messages. Keep **WebSocket** for browsers
and **mutually authenticated TLS/TCP** for peers. The migration's macOS repository and real-browser
acceptance gates pass; see the [roadmap](../ROADMAP.md). The performance
measurements below still describe the frozen benchmark adapters and baseline
application, rather than timings of every subsequent production change.

The [adopted standard](WIRE_STANDARD_PROPOSAL.md) explains compatibility and
rollout. The [application contract](../protocol/README.md) and
[peer contract](../crates/fern-cluster/protocol/README.md) define the actual fields,
limits and parser policies. This is the checklist application's shared message
contract; generation of codecs for arbitrary Fern application types remains work.

## Why JSON was kept, and why the decision changed

JSON was a working baseline with exact integer handling, readable diagnostics,
closed schemas and already-tested delivery behavior. The original native-only
benchmark established smaller binary payloads, but had no browser/WASM boundary
or compiled-application measurements. Keeping it then avoided claiming a benefit
we had not measured. It did not establish that JSON was free or optimal.

The new experiments measure native codec work, real browser/WASM transfers, and
a loopback WebSocket carrying commands through the real Hub and compiled Fern
domain actor. JSON does cost measurable encoding, decoding and conversion work.
Binary encoding saves substantial traffic in the tested model; protobuf also
decodes faster in the tested browser adapters. These observations, plus explicit
field schemas and cross-language tooling, support choosing protobuf as the live
message standard. CBOR remains a strong measured alternative, not a failed format.

The transport and the encoding are separate choices. Protobuf can be sent directly
in a binary WebSocket message; selecting it does not require gRPC, an HTTP/2
service, a proxy or another server process.

## What was measured

All reports preserve raw observations, exact versions, source fingerprints,
correctness checks, workload definitions and limitations. The measurements used
an Apple M4 with 24 GiB RAM and macOS 26.5.1. Agent builds and timed experiments
were coordinated; normal desktop background activity remained. They are local
experiments, not production service-level guarantees.

### Native bytes and codec work

The [native codec experiment](../benchmarks/network-codecs/README.md) compares
46 actual client/server message fixtures. An Add command occupies
**202 / 69 / 66 bytes** in JSON / CBOR / protobuf. A 100-task ASCII snapshot
occupies **6,379 / 2,548 / 2,745 bytes**. A Unicode-heavy snapshot is
**25,379 / 21,648 / 21,947 bytes**: when text dominates, changing field encoding
saves a smaller fraction. These are application payloads, without transport
headers or compression.

The binary adapters preserve native signed 64-bit integers, required-field
presence and conversion to the real protocol values. They are measured as
implementations, not as intrinsic limits of their formats. Production promotion
uses a separately bounded closed profile; prototype measurements do not prove
that every malformed input has identical acceptance across codecs.

### Browser/WASM decoding and transfer

The [real-browser experiment](../benchmarks/network-codecs/BROWSER.md) uses
Microsoft Edge 153.0.4234.32. JSON crosses the actual JavaScript-string boundary;
binary codecs cross byte-array boundaries. Median encode / decode-and-validate
times, including those transfers, are microseconds per message:

| Message | JSON | CBOR | Protobuf |
| --- | ---: | ---: | ---: |
| Add command | 0.703 / 1.120 | 0.391 / 0.478 | 0.350 / 0.333 |
| 100-task ASCII snapshot | 12.054 / 26.803 | 3.362 / 11.071 | 3.751 / 7.301 |
| 100-task Unicode snapshot | 48.287 / 51.181 | 5.466 / 35.035 | 5.991 / 19.773 |
| 100-task escape-heavy snapshot | 145.600 / 210.400 | 6.438 / 14.837 | 6.936 / 9.345 |

For the ASCII snapshot, protobuf decoding is about **3.7× faster** than JSON in
this experiment. Escape-heavy text deliberately stresses JSON escaping; it is
not representative of all application traffic. CBOR encodes the larger snapshots
slightly faster than protobuf and sometimes uses fewer bytes; protobuf is the
stronger decoder across these examples.

The complete JSON / CBOR / protobuf experiment WASM modules are
**242,416 / 210,436 / 152,422 bytes** raw and
**92,326 / 73,810 / 62,549 bytes** with gzip. They contain fixture data and
benchmark exports, and JSON has extra string-boundary controls. They are **not**
the incremental production cost of adding a codec. The actual application still
needs JSON for HTTP and local storage. Page-granular linear-memory capacities
are recorded, but do not measure live allocations, total browser RSS or leaks.

These are repeated batch means with warm input, not per-message p99 latency.
They exclude WebSocket/TLS transport, DOM rendering and cold navigation.

### A real compiled application over WebSocket

The [message-path experiment](../benchmarks/message-path/README.md) performs
**7,650 measured mutations in 90 cases**, using the real Hub, native Fern actor
and optional checkpoint writer. Independent checks validate every returned task,
sequence, revision and identity, then reopen durable state. Median round trips:

| Case | JSON | CBOR | Protobuf |
| --- | ---: | ---: | ---: |
| One task, ephemeral | 81.6 µs | 71.8 µs | 62.8 µs |
| 100 tasks, ephemeral | 2.0477 ms | 2.0506 ms | 2.0460 ms |
| 100 tasks, durable | 12.972 ms | 12.898 ms | 12.955 ms |

The larger cases show essentially unchanged complete-path latency. Small cases
vary between repetitions, so this does not establish a precise application
speedup caused by the codec. The 100-task ephemeral command/outcome/snapshot
payload averages **4,692 / 1,614 / 1,669 bytes**, approximately **66% / 64% less
traffic** for CBOR / protobuf than JSON in this workload.

Server command decoding plus reply encoding averaged **8.86 / 4.64 / 4.77 µs**
for JSON / CBOR / protobuf in that case. These represent
**0.44% / 0.23% / 0.23%** of summed decode + Hub + snapshot-copy + encode
**wall-time intervals**. They are not sampled CPU-utilization shares. The nested
native-domain interval averaged about **2.01–2.04 ms**, and includes compiled
actor execution and the native JSON bridge. Durable runs also include checkpoint
work. The experiment does not separate those internal costs; calling the entire
interval “JSON time” would be wrong.

The process contains one native client and one owner thread. It excludes
production authentication, Origin checks, clustering, fan-out, browser/WASM glue,
DOM work, TLS and remote networks. Thus it demonstrates real application work
and socket traffic, but is not a complete production-server capacity benchmark.

## Chosen format and compatibility

| Boundary | Standard |
| --- | --- |
| Browser/WASM live connection | Binary WebSocket, `fern.live.protobuf.v1` |
| Explicit legacy browser compatibility | Text JSON, `fern.live.v1` |
| Server forwarding | Bounded length-prefixed protobuf over mutual TLS; ALPN `fern.peer.protobuf.v1`, handshake version 2 |
| HTTP, admin JSON, node configuration, offline records and checkpoints | Their existing independently versioned JSON formats |

The application's logical version remains 1. Native `i64` values use protobuf
`sint64` without a JavaScript Number intermediate. JSON compatibility retains
canonical decimal strings. Ordinary JSON numbers cannot portably represent all
signed 64-bit integers; RFC 8259 identifies ±(2^53−1) as the interoperable integer
range, and ProtoJSON also represents 64-bit integers as strings.
[JSON numeric interoperability](https://www.rfc-editor.org/rfc/rfc8259.html#section-6),
[ProtoJSON](https://protobuf.dev/programming-guides/json/).

Fern's profile is deliberately closed. It rejects unknown fields, duplicate
singular fields, absent required values and irrelevant variant fields. Generic
protobuf normally offers different unknown-field and merging behavior. The schema
and structural budget validator are both part of Fern's contract. Stable field
numbers do not by themselves provide rolling-upgrade compatibility.
[Protobuf encoding](https://protobuf.dev/programming-guides/encoding/),
[presence](https://protobuf.dev/programming-guides/field_presence/).

Clients verify their negotiated subprotocol; receivers never guess an encoding
or retry another parser after malformed input. Old peer ALPN/version pairs fail
explicitly, requiring a coordinated peer upgrade. Wire changes do not rewrite
offline records or checkpoint placement. Reconnection preserves uncertainty and
never automatically replays an uncertain mutation into a fresh namespace.

## Why keep these transports?

WebSocket already supports binary messages and fits ordinary HTTPS deployments.
Its browser API still lacks receive backpressure: queue bounds, send-buffer
checks, coalesced replaceable snapshots and disconnect/resync behavior remain
necessary. Outcomes must not be coalesced like snapshots.
[WebSocket standard](https://websockets.spec.whatwg.org/).

Official gRPC-Web supports unary and server streaming, but not client or
bidirectional streaming. That makes it a poor replacement for this continuous UI
channel. A Rust server can embed gRPC-Web translation, so an external Envoy
process is not intrinsically required; it does not remove that streaming limit.
[gRPC-Web](https://github.com/grpc/grpc-web),
[tonic-web](https://docs.rs/tonic-web/latest/tonic_web/).

WebTransport is a future transport experiment for independently flowing streams
or replaceable presence data. Safari 26.4 added support in March 2026; describing
it as generally unavailable is outdated. Older clients and network/intermediary
behavior still need coverage. Neither streams nor datagrams supply application
authorization or durable acknowledgement.
[WebKit announcement](https://webkit.org/blog/17862/webkit-features-for-safari-26-4/),
[WebTransport specification](https://w3c.github.io/webtransport/).

Phoenix's serializer uses compact JSON arrays for ordinary messages and a binary
path for ArrayBuffer; LiveView uses Phoenix Socket. That is useful architectural
inspiration, not evidence that JSON is the best representation for Fern's WASM
host. Fern's browser-owned model/update/view is a different rendering design.
[Phoenix serializer](https://raw.githubusercontent.com/phoenixframework/phoenix/main/assets/js/phoenix/serializer.js),
[LiveView socket](https://raw.githubusercontent.com/phoenixframework/phoenix_live_view/main/assets/js/phoenix_live_view/live_socket.ts).

Erlang distribution informs node identity, handshakes, framing and failure
semantics. Its default cookie exchange is not encrypted transport. Fern's peers
authenticate and encrypt their configured connections; changing JSON to protobuf
does not add remote language PIDs, replicated failover or exactly-once effects.
[Erlang distribution](https://www.erlang.org/doc/apps/erts/erl_dist_protocol.html),
[distributed security](https://www.erlang.org/doc/system/distributed.html),
[signal semantics](https://www.erlang.org/doc/system/ref_man_processes.html).

## What should be optimized next?

Profile the native application bridge, actor execution and checkpoint work
separately. Network protobuf leaves the native JSON request/reply bridge intact.
Avoid attributing all domain work to serialization or promising that binary
frames alone multiply server throughput.

Other distinct experiments include encoding an immutable publication once per
negotiated schema for authorized subscribers, removing redundant validation
serialization, and revision-based deltas with explicit resynchronization. Keep
authorization, admission budgets and delivery outcomes unchanged while measuring
each change independently. [FlatBuffers](https://flatbuffers.dev/white_paper/) or
[Cap'n Proto](https://capnproto.org/faq.html) may merit experiments for large
mostly-read buffers. Their buffer-oriented access does not by itself remove the
current browser/WASM transfer boundary or application validation.

The [Fern/Rust/TypeScript comparison](../benchmarks/language-comparison/README.md)
also identifies immutable collection/model work as a significant current Fern
weakness, alongside low process memory and fast small-source builds. It is
another reason to profile the complete application rather than focus exclusively
on wire parsing.
