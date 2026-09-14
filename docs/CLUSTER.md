# Connected Fern servers

Fern's web server can route a browser's messages through one gateway to a room
actor on another server. Every node runs the same Rust executable. Peer links use
mutual TLS, configured node identities and bounded, length-prefixed messages.
No broker, Erlang installation, discovery daemon or external certificate tool is
required for the local demo.

This is fixed room ownership across servers. Fern language `Pid` values remain
local: remote spawn, links/monitors, dynamic membership, replicated failover and
distributed transactions are separate work. A disconnected owner stays the owner.

## Run three nodes

Build the embedded application with `cargo xtask web-build`, then initialize a
new private directory under a parent you own:

```sh
./dist/fern-web --cluster-init ./fern-cluster-demo demo \
  a=127.0.0.1:4400 b=127.0.0.1:4401 c=127.0.0.1:4402
```

The command prints the settings path for each node. It creates a private CA,
distinct certificates and private node bundles, and never overwrites an existing
destination. The CA private key is not retained in the runtime bundles. Nodes
are sorted by ID; these three settings files are `node-0/node.json` through
`node-2/node.json`.

Run each command in a separate terminal:

```sh
FERN_WEB_CLUSTER=./fern-cluster-demo/node-0/node.json \
FERN_WEB_BIND=127.0.0.1:4100 FERN_WEB_DATA_DIR=./fern-data-a \
FERN_WEB_ACCESS_KEY='choose-a-long-local-demo-key' ./dist/fern-web
```

```sh
FERN_WEB_CLUSTER=./fern-cluster-demo/node-1/node.json \
FERN_WEB_BIND=127.0.0.1:4101 FERN_WEB_DATA_DIR=./fern-data-b \
FERN_WEB_ACCESS_KEY='choose-a-long-local-demo-key' ./dist/fern-web
```

```sh
FERN_WEB_CLUSTER=./fern-cluster-demo/node-2/node.json \
FERN_WEB_BIND=127.0.0.1:4102 FERN_WEB_DATA_DIR=./fern-data-c \
FERN_WEB_ACCESS_KEY='choose-a-long-local-demo-key' ./dist/fern-web
```

Open two gateway addresses, sign in, and edit the shared checklist. The browser
connects automatically to its own origin's WebSocket endpoint. Its gateway opens
an authenticated connection to the configured room owner when needed. `/admin`
shows configured versus connected nodes, stream admission and message counters,
alongside the existing process and native memory observations. Configured nodes
include this server; connected nodes count distinct remote node IDs with admitted
streams. Zero connected peers can simply mean no remote rooms are open.

For separate machines, initialize reachable private IP addresses, distribute only
the appropriate node bundle to each machine, and use a separate data directory
per node. Set each node's exact public `FERN_WEB_ORIGIN` when binding its browser
HTTP listener outside loopback. Browser HTTPS/WSS still uses a TLS terminator;
peer TLS is built in. Cluster members are trusted gateways under the preview's
shared-key authorization model, not isolated tenants.

## Ownership and delivery

All nodes agree on one bounded manifest with 1–16 members. A versioned rendezvous
hash over the room and sorted node IDs chooses its owner. Health does not change
placement. TLS verifies the CA, server name, mandatory client certificate, ALPN
and the configured certificate fingerprint. The handshake then checks protocol,
cluster, full manifest and node identity. A different boot of a node cannot
replace another boot while its existing streams remain live.

Each remote browser subscription has its own persistent TLS forwarding stream.
Commands preserve order on that stream. Outcomes have their own bounded queue;
complete snapshots may coalesce. Native heaps, closures and PIDs never cross the
connection. This first implementation does not multiplex every subscription into
a single socket per node pair.

A socket write is not a commit acknowledgement. Only an application outcome
confirms a mutation; when checkpoints are enabled, persistence precedes that
outcome. If the response is lost, completion is uncertain. A new forwarding
stream gets a fresh namespace and never automatically replays the old mutation.
The browser retains its draft and reports uncertain work for review. Duplicate
commands on the same live namespace use the existing bounded outcome history.

Logout immediately stops new gateway admission. Its stream closes and invalidates
the owner's delegated capability; queued native work checks that capability
before executing. Already admitted work can finish. A partition cannot promise
instant remote revocation: transport deadlines and the bounded delegated lifetime
retire the stream. No remote wall-clock timestamp is trusted.

Each owner checkpoints only its own rooms. The checkpoint directory records its
cluster, node and placement identity and rejects accidental reuse with another
placement or as a standalone server. Existing nonempty standalone data requires
explicit migration. Membership changes require offline data migration and a
coordinated deployment. Certificate renewal and address changes retain placement
when cluster and member IDs stay the same; all peers must still agree on the new
full handshake manifest. Restarting the same owner restores acknowledged tasks
under a fresh incarnation; it does not recover other nodes' data.

## Limits and failure behavior

| Resource | Current bound |
|---|---:|
| Configured nodes | 16 |
| Inbound / outbound forwarding streams per process | 64 / 64 |
| Concurrent peer handshakes, shared across directions | 8 |
| Browser command or event | 64 KiB |
| Framed peer record, including metadata | 68 KiB |
| Queued commands per gateway stream | 1, plus one awaiting its outcome |
| Non-coalesced outcome queue per subscription | 16 |
| Snapshot queue per subscription | One replaceable snapshot |
| Owner ingress, shared with local workers and authentication | 256 requests |
| Browser join / peer write deadline | 2 seconds |
| Peer TLS handshake / admitted command outcome deadline | 10 seconds |
| Nominal ping check interval / frame read deadline | 10 / 30 seconds |
| Delegated stream lifetime | Remaining session lifetime, capped at one hour |

The command deadline starts on admission; a queued command can wait first.
Heartbeat checks run on a one-second cooperative tick: an outstanding ping at
the next nominal ten-second check closes the stream. This is not a strict RTT
guarantee.

These are admission limits, not throughput promises. Slow readers cannot create
an unbounded message queue. Exhaustion closes or rejects the affected connection;
it never routes writes to a replacement owner. The memory bound includes multiple
queues and TLS buffers, not just the size of one message. Admin memory samples
are observations of a process, not a globally atomic cluster measurement.

## Reproduce the checks

```sh
cargo test -p fern-cluster
cargo test -p fern-web --test cluster_stress -- --nocapture
cargo test -p fern-web --test cluster_adversarial
cargo test -p fern-web --test cluster_cli
cargo xtask check
```

The core suite uses an independent virtual-clock state oracle and exact replay.
Real TLS tests exercise framing, cancellation, hostile lengths, authentication,
certificate/node mismatches and 10,000 frame round trips. The separate process
suite drives three actual servers through an opaque TCP fault relay: multiple
gateways, exact authoritative state, lost responses, partitions, slow readers,
durable owner restart and namespaces that do not replay uncertain mutations.
Simulation time describes scheduled scenarios, not equivalent production uptime.

On 2026-09-14, the debug macOS ARM64 acceptance run applied 10,024 durable
mutations across 32 clients and eight rooms in 92.885 seconds (107.9 applied
operations/second). Send through committed outcome and all four matching observer
snapshots took p50 57.184 ms, p95 97.065 ms and p99 105.263 ms. This includes real
checkpoint synchronization and runs in the test harness alongside development
work; it is neither a release-build throughput benchmark nor a network SLA.
The full 98.46-second scenario also exercised balanced owners, 256 updates with
an unread browser, a partition with a healthy sibling owner, and durable restart.

See the [wire-format research and measurements](NETWORK_PROTOCOL.md) for why the
browser keeps WebSocket + JSON while CBOR and protobuf remain measured candidates.
