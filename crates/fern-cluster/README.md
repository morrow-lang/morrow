# fern-cluster

Fixed membership, authenticated forwarding transport and private node provisioning for Fern Web. The crate has no compiler or native actor dependency. Each remote browser subscription gets one TLS stream to its configured room owner; Fern Web supplies process-wide admission, authentication watches and application queues.

Rooms use SHA-256 rendezvous routing over validated membership. Ownership is independent of connectivity. Peers verify the same manifest, TLS 1.3 mutual certificate authentication, `fern.peer.v1` ALPN, the expected server DNS name and a distinct configured leaf-certificate fingerprint for each node. Boot and stream IDs are fresh nonzero 128-bit values supplied by the host. Identifiers never become credential paths or certificate names.

The closed v1 wire carries Join, Command, Event, Ping, Pong and Close. Join carries the gateway's remaining authenticated lifetime, capped at one hour; transport inactivity is independently limited to 30 seconds. It carries no browser cookie, access key, callback pointer or resume namespace. A new forwarding stream receives a new application command namespace. Transport does not replay commands, acknowledge domain commits, elect replacement owners or provide remote native PIDs. The owner remains the checkpoint authority.

`connect` and `accept` return authenticated `PeerStream` read/write halves. Acquire process-wide admission before either call and retain it for the stream lifetime. `FrameReader::read` retains partial progress and its absolute deadline when cancelled. Cancelling a partial write poisons the writer; drop the stream. Frame lengths are checked before allocation, each body buffer is capped at 69,632 bytes, and the existing inner application frame remains capped at 65,536 bytes. TLS handshake/read/write deadlines include a final wall-clock check so buffered IO cannot silently complete after expiry.

`provision(new_directory, cluster, nodes)` atomically publishes up to sixteen separate node bundles with fresh certificates, DER private keys and closed JSON configuration. `NodeSettings::load` returns routing, TLS security and a peer bind address. Provisioning requires an existing trusted parent that is not group/other-writable; another process under the same account is inside that trust boundary. The destination must not exist. Directories use mode 0700 and files 0600, with directory-relative exclusive creation, no symlink following, inode-checked bounded cleanup and no-replace publication. Signing material for the cluster CA is never written to disk. Copy each node's own bundle to its server; runtime nodes need no CA signing key or OpenSSL command. Existing bundles are not silently regenerated or rotated.

```sh
cargo test -p fern-cluster
cargo clippy -p fern-cluster --all-targets -- -D warnings
```

The suite covers an independent seeded queue oracle, deterministic routing and identity rejection, cancellation/deadline/frame bounds, credential filesystem failures, real provisioning-to-TLS connections, missing/foreign/forged peer credentials and 10,000 real TLS command-envelope roundtrips. Those roundtrips test transport identity and ordering; application durability, logout and multi-process fault campaigns belong to Fern Web's acceptance suite.

The TLS provider is the project's existing mature rustls/ring stack; this includes a wrapped third-party native cryptographic dependency. Fern's implementation remains Rust.
