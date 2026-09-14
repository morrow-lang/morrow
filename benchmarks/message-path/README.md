# Compiled Fern application over a loopback WebSocket

This standalone experiment changes only the network encoding around the actual
`fern-web-protocol::Hub` and `fern-web-app::NativeDomain`. The domain executes the
compiled Fern actor in `examples/web/server.fn`. It compares current JSON text
WebSocket messages with the isolated CBOR and protobuf binary adapters from
`benchmarks/network-codecs`.

```sh
cargo test --manifest-path benchmarks/message-path/Cargo.toml --locked
cargo clippy --manifest-path benchmarks/message-path/Cargo.toml --locked --all-targets -- -D warnings
cargo build --release --manifest-path benchmarks/message-path/Cargo.toml --locked
target/release/fern-message-path 150 20 5 > message-path-results.json
```

Arguments are measured operations per ephemeral case, operations per durable
case, and repetitions. Defaults are 150, 20 and 5. Use the same Cargo target
directory consistently when building and locating the executable. The prototype
currently uses Unix directory ownership checks, like the native server on the
measured host.

## Workload and independent oracle

Each case owns a new loopback connection and native owner thread. It first adds
1, 25 or 100 tasks over the same WebSocket outside the measured samples. It then
toggles tasks in round-robin order. Every command receives both an Applied outcome
and a full authoritative snapshot. The client independently computes every task
ID, label, boolean, sequence and revision, and checks the exact returned state and
namespace/incarnation. Checks run after recording each RTT. Durable cases reopen
the real checkpoint directory after dropping the owner and verify recovered
state and the next task ID.

Three untimed cases warm the runtime, codecs and socket code. Codec order rotates
by repetition, persistence mode and task count. Every case has a fresh domain and
state; no candidate inherits another candidate's mutated model. Durability uses
the existing checkpoint writer's real synchronization and atomic publication.
No fake filesystem or success response replaces the application.

The test-first acceptance suite exercises all three codecs in both persistence
modes with an independent three-task/seven-toggle oracle. It also rejects invalid
task/operation bounds before creating a server. These tests complement the
adapter correctness suite; this experiment does not provide its own alternative
serialization implementation.

## Timing interpretation

Each raw sample records:

- Client RTT: before command encoding through reading and decoding both replies.
  Command construction and independent output assertions are outside this interval.
- Client encode and combined reply decode times, plus application payload bytes.
- Server command decode, `Hub::command`, snapshot copying, encoding both replies,
  and WebSocket write/flush times.
- The nested `NativeDomain::apply` interval within `Hub::command`. This includes
  the existing native actor JSON bridge and, in durable mode, checkpoint work.

`native_domain_ns` is included in `hub_ns`; do not add them together. Socket reads
and TCP/WebSocket framing are outside the server decode timer. JSON's conversion
from encoded bytes to a WebSocket text value is inside encoding. The current
binary adapters include their DTO conversion and structural validation costs.
Instrumentation uses `Instant` around short stages; timer overhead is present for
all candidates and matters particularly for sub-microsecond stages.

The reported p50/p95/p99 use nearest-rank per-command observations. Durable
defaults produce only 20 observations per case, so p95/p99 are not stable tail
estimates. Keep the raw samples and repeated-case variation. A sum of RTTs can
describe this one-outstanding-command client, but is not a many-client server
capacity measurement. Payload bytes exclude WebSocket headers, client masking,
TCP/IP and TLS overhead. Buffers, 64 KiB frame limits, TCP_NODELAY, and explicit
flushing of the two replies are identical for all candidates.

## Scope

This is a real socket and real compiled-application experiment, **not the complete
production server**. It deliberately excludes authentication, origin checks,
cluster forwarding and its extra validation encodes, peer/browser TLS, remote
network conditions, fan-out, browser/WASM glue, DOM rendering and reconnects.
Those costs must not be inferred from these measurements. Server and client run
in one native process on separate threads; this is neither a browser latency
benchmark nor a comparison between Fern and another language.

The native application bridge and durable checkpoint format remain JSON for all
candidates. A binary network message does not remove them. This lets the nested
domain timer show how much of the observed path remains unchanged by a codec
selection.

Resources are bounded: one connection, five-second TCP IO timeouts, fixed schema
and task limits, bounded operation/repetition arguments, and one owner thread
joined before returning. Each durable case creates a private directory
exclusively and removes it only while its directory identity still matches.
No production codec or transport negotiation is enabled by this package.

## Observed September 14, 2026 run

Apple M4, 24 GiB, macOS 26.5.1; Rust 1.100.0-nightly
`f248f4038 2026-09-05`; optimized release, thin LTO and one codegen unit. A
coordinated quiet slot excluded other agents' builds and timed experiments;
ordinary OS background activity was not controlled. The 90 cases contained 7,650
measured mutations, plus untimed initialization and warmup. All exact transition
and durable reopen checks passed.

Pooled per-command median round-trip times across five repetitions:

| Persistence | Tasks | JSON | CBOR | Protobuf |
| --- | ---: | ---: | ---: | ---: |
| Ephemeral | 1 | 81.6 µs | 71.8 µs | 62.8 µs |
| Ephemeral | 25 | 153.0 µs | 154.5 µs | 149.8 µs |
| Ephemeral | 100 | 2,047.7 µs | 2,050.6 µs | 2,046.0 µs |
| Durable | 1 | 9.018 ms | 8.052 ms | 8.987 ms |
| Durable | 25 | 10.129 ms | 9.995 ms | 9.978 ms |
| Durable | 100 | 12.972 ms | 12.898 ms | 12.955 ms |

The larger application cases show essentially unchanged observed latency across
codecs. Small cases varied substantially between repetitions: for example the
one-task JSON batch medians ranged from 51.6 to 106.0 µs. These observations do
not support a precise codec-caused improvement in complete application latency.

For 100-task ephemeral mutations, the measured native-domain interval averaged
about 2.01–2.04 ms. Server command decoding plus reply encoding averaged
8.86/4.64/4.77 µs for JSON/CBOR/protobuf. That is 0.44%/0.23%/0.23% of the summed
decode + Hub + snapshot-copy + encode **wall-time** intervals. These are not
sampled thread-CPU utilization measurements. The native-domain interval includes
compiled actor execution, its JSON bridge and, when enabled, checkpoint work;
the experiment does not isolate those internal costs from one another.

For the same 100-task ephemeral workload, combined command/outcome/snapshot
payloads averaged approximately 4,692/1,614/1,669 bytes: CBOR and protobuf reduced
payload traffic by about 66% and 64%. Those sizes depend on the exact task labels,
IDs, revisions and boolean state. They do not predict text-heavy applications or
include lower-level framing overhead.

This experiment supports binary encoding for bandwidth savings, together with
the separate browser measurements. It does **not** establish network JSON as the
current compiled application's dominant bottleneck. The native application
bridge, actor runtime and checkpoint path deserve separate profiling before
claiming or attempting large server throughput improvements.

Raw samples: [results-20260914.json](results-20260914.json).
Aggregates with per-repetition medians and raw stage totals:
[summary-20260914.json](summary-20260914.json).
Source/dependency fingerprints, executable hash and host metadata:
[measurement-metadata.json](measurement-metadata.json).
