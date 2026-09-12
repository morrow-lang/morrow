# Fern web preview

The collaborative checklist connects a real Fern WebAssembly module, a Rust
browser host and an authenticated Rust HTTP/WebSocket server. It demonstrates
local interaction, shared confirmed state and offline reload in one deployable
server executable. It is an ephemeral application preview, not yet a complete
Fern web framework or a distributed actor system.

## Build and run

Use the repository's pinned Rust toolchain and the native build prerequisites
in [BUILD.md](../BUILD.md). Install the browser target and matching binding tool:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
cargo xtask web-build
FERN_WEB_ACCESS_KEY='replace-with-a-long-random-secret' ./dist/fern-web
```

Choose your own random access key of 16–256 bytes. Open
**http://127.0.0.1:3000**, enter the key and connect a second window to the same
address. Both clients use the `garden` room. Add a task, toggle its checkbox and
change the local filter. All users of this preview key have the same room access;
there is no account or tenant administration system.

`web-build` compiles the Fern example, the Rust browser host and the Rust service
worker, runs the pinned `wasm-bindgen`, and embeds the generated assets in the
server. No Node/npm toolchain is required. Set `WASM_BINDGEN` to an explicit
executable path if the matching tool is not on `PATH`.

The default output is `dist/fern-web`. An optional positional path selects another
output: `cargo xtask web-build /path/to/fern-web`. The build publishes the finished
executable after all components succeed. Run `fern-web --help` for configuration
and `fern-web --licenses` for the embedded third-party notices.

To compile just the example's Fern functions after a normal compiler build:

```sh
./bin/fern build --target=wasm32 examples/web/checklist.fn -o checklist.wasm
```

This is a library module with exported functions, so it does not need `main`.
The browser build fetches and executes this compiler output as `fern_app.wasm`.

## Ship a static Linux server

Add the desired Linux musl standard library, then select that target:

```sh
rustup target add aarch64-unknown-linux-musl
FERN_WEB_TARGET=aarch64-unknown-linux-musl cargo xtask web-build
```

This produces `dist/fern-web-linux-arm64`. For x86-64:

```sh
rustup target add x86_64-unknown-linux-musl
FERN_WEB_TARGET=x86_64-unknown-linux-musl cargo xtask web-build
```

This produces `dist/fern-web-linux-x86_64`. The build selects Rust's bundled LLD
linker and verifies the ELF architecture, absence of a dynamic interpreter and
absence of required shared libraries. These Linux outputs embed their browser
assets and notices; deployment needs no asset directory, Fern compiler, Rust
installation or shared libraries. A host build without `FERN_WEB_TARGET` retains
the host platform's normal executable dependencies.

Copy the appropriate binary to a Linux host and configure the public origin:

```sh
FERN_WEB_BIND=0.0.0.0:3000 \
FERN_WEB_ORIGIN=https://fern.example.com \
FERN_WEB_ACCESS_KEY='replace-with-a-long-random-secret' \
./fern-web-linux-arm64
```

Place a TLS terminator in front of the HTTP listener for public HTTPS/WSS. It
must forward WebSocket upgrades and preserve the browser's exact origin. The
origin includes the scheme and any non-default port, with no trailing slash.
Binding outside loopback requires `FERN_WEB_ORIGIN`; loopback defaults to the
listener's HTTP origin. `/health` provides a small readiness response.

The static server is self-contained; automatic clustering, durable storage and
built-in TLS termination are separate features. The web server is a separate
workspace package, so ordinary CLI programs do not acquire its transport or
browser dependencies.

For a separate lightweight measurement, `examples/tiny_cli.fn` built with the
2026-09-12 release compiler/runtime produced a **566,056-byte (553 KiB)** macOS
ARM64 executable and printed `hello, fern`. It linked only the platform's
`libSystem` and `libiconv`, with no web server or browser bundle. This is one
measured example, not a general size budget or a static Linux CLI claim.

## Offline and reconnect behavior

After a successful online visit caches the assets, the Rust service worker can
reload the application offline. Service-worker availability requires a supported
browser and a secure context: HTTPS, or loopback development. The browser keeps
a bounded local draft and last confirmed snapshot in local storage. Filtering
and draft editing work locally; confirmed shared data is read-only while offline.
The first visit requires a connection, and browsers can evict local storage or
caches. This is useful local continuity, not a durable backup.

Only one shared mutation can be outstanding per client. A pending command is
distinct from a confirmed snapshot. Reconnecting obtains a current snapshot and
can resolve an outcome within the server's bounded session window. A reload
restores the draft and snapshot, but does not persist authentication secrets,
command namespaces or a mutation replay queue. Uncertain completion is shown for
review; offline edits are not blindly replayed against fresh server state.

Room state lives in memory and disappears when the server process restarts.
A new resource incarnation invalidates old commands. Logout revokes the server
session and its active connections, and clears the client's saved server snapshot.
Cached application files and local drafts
are browser-local data, not evidence of a still-valid server session.

## Protocol and limits

The shared Rust protocol uses versioned, closed JSON schemas, canonical decimal
strings for i64 values and separate resource, command-namespace and connection
identities. Commands carry an expected revision and monotonic sequence number.
The server resolves retained duplicates before revision conflicts, rejects
changed payloads for retained command identities and never executes an old
sequence again after its cached outcome expires. Unknown completion and resets
are explicit outcomes; this does not provide durable exactly-once effects.

The default room holds at most 100 tasks with labels of at most 256 UTF-8 bytes.
Frames are limited to 64 KiB. Connections, rooms, sessions, queued commands and
retained outcomes have aggregate limits. Replaceable snapshots are coalesced;
command outcomes are queued separately, and slow writers are disconnected.
Absolute join deadlines, heartbeats and bounded reconnect backoff limit stale
connections. See the defaults in
[`fern-web-protocol`](../crates/fern-web-protocol/src/lib.rs) and
[`fern-web`](../crates/fern-web/src/lib.rs).

Sign-in exchanges the access key for an HttpOnly, SameSite=Strict cookie and a
CSRF nonce. WebSocket upgrades check the exact Origin, session and CSRF protocol;
operations recheck session validity. Public deployment needs HTTPS. This shared
key mechanism is deliberately a preview authentication model; per-user resource
authorization and operational administration remain application work.

## What executes where

| Component | Current responsibility |
| --- | --- |
| [`examples/web/checklist.fn`](../examples/web/checklist.fn) | Compiled Fern policy: filtering, submit eligibility, completion percentage and toggle behavior |
| [`fern-browser`](../crates/fern-browser) | Rust WASM host: model storage, wire transport, keyed accessible DOM, focus, drafts and lifecycle cleanup |
| [`fern-browser-worker`](../crates/fern-browser-worker) | Rust service worker: versioned static-asset caching and offline loading |
| [`fern-web-protocol`](../crates/fern-web-protocol) | Portable Rust wire types, reconciliation and authoritative ephemeral checklist state |
| [`fern-web`](../crates/fern-web) | Axum/Tokio transport, authentication, bounded state owner and embedded assets |
| [`fern/src/wasm`](../crates/fern/src/wasm.rs) | Separate compiler backend with scalar values and a bounded precise String heap |

The checklist's scalar Fern module has no host imports or shared linear memory.
The Rust host invokes its checked exports with i64 values represented as browser
BigInts. The separate String heap is compiler functionality; it is not required
by this scalar example. Generated loader/binding JavaScript is a build artifact.

The server's authoritative model currently executes as Rust behind a serialized
owner task. It is not a compiled Fern domain actor. Native Fern actor heaps and
message copying are a separate runtime foundation. Moving the complete model,
update and view into typed Fern and connecting real Fern server actors are open
integration steps. Fair resumable scheduling, typed supervision, multicore
execution, durable recovery and clustering remain later gates.

## Verification

Run the reproducible browser check against a built server with a Chromium/Edge
executable available:

```sh
FERN_BROWSER=/path/to/chromium cargo xtask web-check dist/fern-web
```

The runner starts its own server and browser sessions. Set `FERN_WEB_SCREENSHOT`
and `FERN_WEB_MOBILE_SCREENSHOT` to output paths to retain desktop and mobile
screenshots. Use the appropriate static binary path when checking a Linux build
on a matching architecture.

The 2026-09-12 final macOS `web-check` passed with two real browser clients:
task creation, completion, compiled Fern policy, local filtering, preserved input
focus, cold service-worker restart, cached offline reload, restored drafts,
reconnect and session revocation. At 320 CSS pixels the mobile layout had no
horizontal overflow; desktop and mobile screenshots were also inspected.
A tampered asset returned with HTTP 200 was rejected during cache update, and
the previously cached application still booted offline. This exercises the
service worker's per-asset integrity checks and preservation of the working cache.

An ARM64 musl binary also served the two-client/offline flow while running
unprivileged in an otherwise empty Linux chroot. The x86-64 musl binary passed
static ELF validation; execution on x86-64 has not been verified. A CI job now
builds the static x86-64 server and runs the browser acceptance, but no GitHub
execution of that new job is claimed here.

The final stripped release artifacts below include the application assets, full
Fern license and third-party notices. All three embed browser asset revision
`0d084490cbb8ff2ae057124082f998bfd7f50aa003839cd10ca658d2dcacd0a2`.
MiB uses 1,048,576 bytes. These are the complete preview server sizes, not compiler
sizes or a general Fern application size guarantee.

| Artifact | Bytes | MiB | Verification |
| --- | ---: | ---: | --- |
| `dist/fern-web` — macOS ARM64 | 2,362,896 | 2.25 | Executed; browser acceptance; host dependencies limited to libSystem and libiconv |
| `dist/fern-web-linux-arm64` | 2,393,256 | 2.28 | Static ELF; final binary executed as UID 65534 in an empty Linux chroot |
| `dist/fern-web-linux-x86_64` | 2,623,896 | 2.50 | Static ELF; not executed |

Both Linux files have no dynamic interpreter or required shared libraries. The
final ARM64 artifact passed help, full-license output, health and HTTP-200 UI
checks in the chroot, with matching host/guest SHA-256. The earlier two-client
browser flow and the final artifact smoke checks are separate validation steps.

Independent protocol and transport tests cover malformed and oversized input,
revision conflicts, duplicates, payload mismatch, expired outcomes/namespaces,
connection replacement, reset, revocation and absolute join deadlines. Compiler
tests execute generated WASM against independent expected results. These checks
do not satisfy the architecture's full typed-actor, aggregate browser ABI,
failure-isolation or scaling gates. The earlier native migration acceptance
record is separate from this new preview work.

The new macOS ARM64 `cargo xtask check` passed workspace tests, formatting,
Clippy, dependency-notice validation, 305 native-output fixtures, 18 examples,
63 dynamic compatibility programs, 295 atomic rejections, 64 grammar fuzz cases
and 192 mutation fuzz cases. Linux ARM64 passed the same coverage through its
workspace check and a resumed acceptance tail after host disk exhaustion made the
guest filesystem read-only. That interruption was recovered; the result is
completed coverage across resumed runs, not one uninterrupted Linux command.
The latest license CLI and dependency-notice checks also passed. Focused new
execution checks passed 17 WASM cases, 17 native ABI cases and four compiler-root
cases. These results do not imply a full optimized compiler-suite run.

See the [architecture](FULL_STACK_ARCHITECTURE.md) for the complete acceptance
contract and the [roadmap](../ROADMAP.md) for remaining work. No comparative
throughput, latency, binary-size budget or percentage-of-vision claim is implied.
