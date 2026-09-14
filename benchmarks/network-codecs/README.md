# Fern network codec measurements

This excluded Cargo workspace compares the current JSON codec with experimental
CBOR/protobuf adapters for actual `fern-web-protocol` messages. Production does not
depend on this benchmark workspace. The measured adapters remain frozen as
experimental references; protobuf was subsequently promoted separately into
`fern-web-protocol::binary` with stricter preallocation limits. An equivalence test
checks that all 46 measured fixtures retain the same production protobuf bytes.

The [real browser/WASM experiment](BROWSER.md) measures the same adapters across
actual JavaScript/WASM boundaries and records separately linked artifact sizes.

```sh
cargo test --manifest-path benchmarks/network-codecs/Cargo.toml --locked
cargo clippy --manifest-path benchmarks/network-codecs/Cargo.toml --locked --all-targets -- -D warnings
cargo run --release --manifest-path benchmarks/network-codecs/Cargo.toml --locked -- 100 9 > codec-results.json
```

The two arguments are iterations per batch and repeated samples. The runner bounds
both, verifies every fixture before timing, warms up, rotates codec order by fixture
and retains raw batch-mean samples. Output destruction is included. Batch medians
are not per-message p99 latency. `std::hint::black_box` prevents unused results from
being discarded. Native timings do not establish browser footprint, network
throughput, allocation counts or end-to-end application speed.

## Observed September 14, 2026 run

Apple M4 (10 CPU cores), 24 GiB memory, macOS 26.5.1 (25F80),
`rustc 1.100.0-nightly (f248f4038 2026-09-05)`, release/thin LTO/one codegen unit,
100 iterations per batch, nine samples, 46 fixtures. Other project work ran
concurrently: treat timing values as illustrative observations, not uncontended
performance claims. Encoded byte counts are deterministic for the recorded corpus.
Raw results and source/dependency fingerprints are checked in alongside this file.

| Actual fixture | JSON bytes | CBOR bytes | Protobuf bytes |
| --- | ---: | ---: | ---: |
| Add command | 202 | 69 | 66 |
| SetDone command with full-width ID | 218 | 64 | 60 |
| Applied outcome | 160 | 50 | 47 |
| 100-task ASCII snapshot | 6,379 | 2,548 | 2,745 |
| 100-task Unicode snapshot | 25,379 | 21,648 | 21,947 |
| 100-task escape-heavy snapshot | 56,579 | 27,348 | 27,547 |

Selected median encode/decode-and-validate times, in nanoseconds per operation:

| Actual fixture | JSON encode/decode | CBOR encode/decode | Protobuf encode/decode |
| --- | ---: | ---: | ---: |
| Add command | 887 / 956 | 558 / 521 | 383 / 514 |
| 100-task ASCII snapshot | 8,544 / 15,949 | 3,347 / 7,681 | 3,977 / 6,647 |
| 100-task Unicode snapshot | 14,806 / 26,974 | 4,495 / 26,408 | 4,420 / 15,675 |

These fixtures demonstrate reduced bytes for the tested binary schemas. They do
not establish a universal winner: text-heavy messages retain most payload bytes,
JSON escaping strongly changes results, and CBOR/protobuf trade positions across
operations. This initial native-only evidence motivated retaining WebSocket/JSON
while evaluating integration cost, real browser performance and transport
separately. The subsequent [browser measurements](BROWSER.md) supply that missing
browser evidence.

## What is measured

JSON calls the real `fern_web_protocol::encode`/`decode`, including its capped
writer and closed Serde schema. Binary paths map `Decimal.0` to native signed
integers and reconstruct the actual message types exactly. Snapshot revisions and
task IDs are tested at both signed extrema and beyond JavaScript's exact-number
range; the envelope corpus exercises the other integer fields. Those codec
fixtures do not imply negative sequences are valid Hub commands.

The complete corpus covers Join/resume, every mutation, Connected, Snapshot,
Reset, every outcome status and every protocol Error. Snapshots contain 0/1/10/100
tasks with ASCII, multibyte and maximum escape-heavy labels. The driver uses
synthetic deterministic values, not a live Hub/network workload.

For binary codecs, separate measurements report prepared-wire encoding/decoding,
actual-message-to-wire adaptation, and **wire cloning plus adaptation/validation**
back into actual types. The latter includes the clone necessary to reuse a prepared
fixture; it is explicitly not a pure no-copy adaptation measurement. Full encode
and decode numbers include the actual conversion path. Authorization, room limits,
revision checks and mutation effects remain Hub responsibilities outside this
codec benchmark.

`validate.rs` checks duplicate/unknown binary fields, wire scalar types, integer
ranges, definite CBOR containers, depth, cardinality and input/output size limits.
Protobuf/CBOR derive libraries alone do not supply the current closed-schema
semantics. Scalar presence remains explicit: missing false/zero values cannot
silently satisfy a required field. The test suite includes independent expected
JSON/CBOR/protobuf bytes, malformed lengths, duplicates, missing fields, truncation,
wrong direction, integer extrema and Unicode. This acceptance is bounded prototype
coverage, not a general proof of all malformed input equivalence.

## Experimental field schema

`src/schema.rs` is the exact Rust schema. CBOR uses integer-key maps; protobuf uses
field tags with the same numbers. All fields are optional in the decoder DTO to
preserve absence, then validated for the selected variant. The Tasks.items field
is repeated. Protobuf integer fields use `sint64` (ZigZag); versions/status/kinds
use `uint32`. Strings and booleans preserve their ordinary types. No external
`protoc` executable is needed; `prost::Message` derives the wire implementation.

| Message | Fields in ascending tag order, starting at 1 |
| --- | --- |
| Wire | kind, join, command, connected, snapshot, outcome, error |
| Join | room, resume_namespace |
| Command | version, incarnation, namespace, sequence, expected_revision, mutation |
| Mutation | kind, label, id, done |
| Connected | version, connection, namespace, next_sequence, snapshot, resumed |
| Snapshot | version, room, incarnation, revision, tasks |
| TaskList | items |
| Task | id, label, done |
| Outcome | version, incarnation, namespace, sequence, revision, status |

Wire kinds: 1 Join, 2 Command, 3 Connected, 4 Snapshot, 5 Reset (uses snapshot field),
6 Outcome, 7 Error. Exactly one corresponding payload is permitted. Mutation kinds:
1 Add, 2 SetDone, 3 Remove. Outcome statuses: 1 Applied, 2 Conflict, 3 NotFound,
4 Capacity, 5 Unknown. Error payloads retain the current snake_case names.
The CBOR profile rejects indefinite containers. Protobuf may represent an empty
TaskList as an empty nested message; CBOR's generated representation explicitly
contains its empty array. Never compare raw bytes across formats for deduplication.

Protocol design and primary sources: [network protocol](../../docs/NETWORK_PROTOCOL.md).
