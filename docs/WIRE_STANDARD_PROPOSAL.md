# Adopted Morrow live wire standard

Date: 2026-09-14. Status: **protobuf selected**, superseding the preliminary CBOR
recommendation and Decision144's JSON baseline. Implementation and migration
acceptance are tracked separately in the [roadmap](../ROADMAP.md). This filename
is retained so earlier proposal links continue to work.

## Decision

Use one bounded, explicitly versioned protobuf message contract between the
browser's Rust/WASM host and the server, and inside authenticated server
forwarding. The first-party browser uses binary WebSocket subprotocol
**`morrow.live.protobuf.v1`**. An explicitly negotiated legacy text path retains
**`morrow.live.v1`**. Peer links use **`morrow.peer.protobuf.v1`** ALPN with peer
handshake version **2**, preserving their four-byte big-endian frame prefix.

Protobuf is the live encoding; WebSocket and mutual TLS/TCP are the transports.
No gRPC service, intermediary proxy or extra runtime process is required.
Configuration, HTTP/admin APIs, browser offline records and durable checkpoints
keep their existing JSON formats and versions. The native Morrow domain's JSON
request/reply bridge is also a separate boundary and remains unchanged.

The [application schema and closed profile](../protocol/README.md),
[application .proto](../protocol/morrow_wire_v1.proto) and
[peer field registry](../crates/morrow-cluster/protocol/README.md) are the contract.
They supersede the provisional CBOR mapping previously described here.

## Why protobuf

Correctness and bounded resource use come first; encoding speed alone cannot
change authorization, delivery or persistence guarantees. The new evidence
supports protobuf after comparing all three implemented candidates:

- The native and browser corpora preserve exact integers, message identities and
  conversions into actual protocol values. Binary messages are substantially
  smaller for ordinary task snapshots; text-heavy content offers smaller savings.
- In a real browser, protobuf decodes a 100-task ASCII snapshot in **7.301 µs**
  including transfer, versus **26.803 µs** for JSON and **11.071 µs** for CBOR.
  CBOR encodes several large examples slightly faster and can be slightly smaller.
- The complete protobuf experiment module is the smallest tested WASM artifact.
  These are benchmark modules, not production codec-size deltas; the application
  retains JSON for other responsibilities.
- Protobuf supplies an explicit field-number schema and established generated
  client conventions. Morrow still must provide strict validation, stable mappings
  and a deliberate compatibility policy. The schema does not generate arbitrary
  Morrow application codecs today.

The [measurement report](NETWORK_PROTOCOL.md) links raw native, browser and real
compiled-application results. In the 100-task loopback application, switching
codecs barely changes approximately **2.05 ms** ephemeral or **12.9 ms** durable
round trips. It reduces combined payload bytes by roughly **64%** with protobuf.
Thus the decision is supported by browser decoding, bandwidth and schema/tooling,
not a claim that network JSON dominates current server CPU.

The measured native-domain interval includes actor execution, its JSON bridge
and, for durable cases, checkpoint work. Stage wall-time fractions are not CPU
utilization, and the whole interval cannot be attributed to JSON. Allocation
counts, actual production bundle deltas, cold navigation, DOM timing and sustained
many-client throughput still need their own experiments.

## Value, schema and resource contract

The logical live-application version remains **1**; it is independent of the
WebSocket codec name and peer handshake version. Protobuf `sint64` preserves
language integers directly in native/WASM code. JavaScript integrations must use
BigInt or another exact representation, never Number for full-width fields.

Required scalar fields retain presence, including false and zero. Exactly one
selected sum variant and its allowed fields are accepted. Unknown fields,
duplicate singular fields, unknown variants and missing required values reject.
Input bytes, nesting, UTF-8 lengths and decoded cardinalities are bounded before
owned structures grow. Encoding checks source limits before cloning and checks
encoded size before allocating the output buffer. The codec validates structure;
Hub/domain logic still validates authority, revisions and allowed operations.

General-purpose protobuf decoders do not enforce this entire profile. The live
application parser accepts field reordering and bounded valid non-minimal
varints; the outer peer parser has its own documented stricter varint policy.
Neither uses incoming bytes as logical command identity. Namespace, incarnation,
sequence and typed command content govern deduplication. Protobuf does not promise
canonical bytes, even when serialization is described as deterministic.
[Protobuf encoding](https://protobuf.dev/programming-guides/encoding/),
[field presence](https://protobuf.dev/programming-guides/field_presence/),
[non-canonical serialization](https://protobuf.dev/programming-guides/serialization-not-canonical/).

Published field numbers, variant numbers, error names and scalar types are stable
assignments. Removed identifiers must never be reused. Because this first schema
rejects unknown fields, adding fields or semantic behavior requires explicit
negotiation of a supported new schema. Generic protobuf unknown-field conventions
do not provide rolling compatibility for this closed profile.

No pointers, native heap addresses, closures, local handles or language PIDs are
serialized. General serializable Morrow types and generated codecs are future work;
the preview protocol must not be advertised as an arbitrary remote actor ABI.

## Negotiation and migration

The new browser offers and verifies `morrow.live.protobuf.v1`; the server may
explicitly select `morrow.live.v1` only for a client requesting legacy JSON.
The selected codec determines text versus binary messages for that connection.
There is no format sniffing or retry through another parser after a failure.

Old peers using `morrow.peer.v1` do not connect to the protobuf peer ALPN. Upgrade
cluster nodes in a coordinated deployment. Peer mismatches fail before commands
are admitted; a transport update does not authorize a different room owner.
Node configuration, placement digests and checkpoint bindings remain independent
of the encoding. This is not dynamic mixed-version cluster upgrade support.

An old browser's saved draft is not a network frame. Offline data keeps its own
format and recovery rules. A reconnect must preserve pending uncertainty and
must never turn an uncertain action into a new automatically replayed mutation.
Legacy browser retirement will require an explicit compatibility policy and
safe old-tab behavior; selecting protobuf does not silently erase that obligation.

## Promotion gates

The benchmark adapters and their measurements are retained independently from
the production codec. The production profile tightens decoded task limits and
uses explicit stable error mappings. It must pass independent fixed wire bytes,
full-width integers, required-value presence, every variant, hostile declarations,
truncation, UTF-8, duplicate/unknown fields and unchanged-state rejection tests.

Migration requires both selected browser protocols, binary message boundaries,
subprotocol mismatch rejection, cross-codec state convergence and real browser
offline/reconnect checks. Peer acceptance must retain authentication, cancellation,
deadlines, message ordering, bounded queues, lost-response uncertainty, partition
and restart stress tests. Shared protocol traces and deterministic simulation
remain necessary alongside real sockets. Complete repository and browser gates
must pass before claiming the migration fully verified.

The integrated macOS repository gate and real-browser acceptance now pass these
migration checks, including explicit mixed JSON/protobuf gateways and a real
remote owner. The [roadmap](../ROADMAP.md) records the exact verified scope.
This validates the bounded preview contract, not arbitrary distributed Morrow
programs or production performance guarantees.
