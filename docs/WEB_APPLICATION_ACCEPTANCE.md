# Fern application and worker acceptance

Measured on 2026-09-13 from source commit
`f80f55a828215987fd4b35dc1c15c192e08812c2`. This record covers a working bounded
application, native actor lifecycle changes and room workers. It does not declare
language 1.0, a general application framework or distributed-runtime readiness.

## Executable application

The checklist's domain, local model, event update and keyed view execute in typed
Fern. The browser runs compiled WASM through a generic Rust DOM host. The server
links native Fern room actors at build time and embeds browser assets; it does
not ship an interpreter or compiler. Optional room checkpoints commit before
acknowledgement and restore state under fresh incarnations after restart.

Rooms are pinned to independent worker threads with shared ingress and resource
quotas. Authentication can revoke queued commands while a worker executes.
Recursive Unit-tail paths suspend through rooted continuation frames; other
synchronous helpers can still occupy a worker. Reusable identity slots preserve
stale-PID rejection and supervision lineage without the old spawn-lifetime cap.
See [worker behavior](WEB_WORKERS.md) and [actor contracts](ACTOR_RUNTIME.md).

## Integrated macOS ARM64 gate

`cargo xtask check` passed formatting, dependency notices, workspace Clippy,
1,890 Rust tests across 248 suites, 305 native-output fixtures, 18 examples,
63 dynamic compatibility programs, 295 atomic rejections, the union continuation
case, 64 grammar cases and 192 mutations. No oracle was skipped or weakened.

Independent tests include 66,536 actor start/finish cycles returning to the
session's memory baseline after precise collection; stale PIDs retained only in
foreign actor heaps; nonwrapping generation exhaustion; and 100,000 mutual
Unit-tail transitions with collection before every handoff. Native poll tests
cover receive arms, timeout bodies, aliased entries and sibling progress.

Worker tests hold one domain callback behind a deterministic gate while another
worker and authentication progress. They verify queued-command revocation,
global quota ownership, connection transfer at capacity, canceled login/join
cleanup, dropped disconnects and partial worker startup. Shared checkpoint tests
preserve concurrent rooms and reject an outdated state before publication.

The rebuilt macOS server passed real-browser acceptance: two clients, compiled
Fern model/update/view, rejected-effect draft preservation, keyed DOM and focus,
cold service-worker restart, offline reload/draft/filter recovery, 320-pixel
mobile layout, reconnect, revocation and atomic rejection of HTTP-200 cache
tampering while preserving the previous offline application.

## ARM64 Linux execution

The same source checkpoint passed 98 focused optimized tests on actual ARM64
Linux: 73 core runtime tests, two checkpoint unit tests, five checkpoint
integration tests, five native-domain tests, seven real transport tests and six
existing compiler-tail oracles. The last group used the existing Fern source
and assertions with a host driver that cross-emitted ARM64 objects, cross-linked
the Rust harnesses and executed the resulting binaries in Linux. This was not a
comparison against another implementation of the lowering.

The final static server passed the full browser suite in Linux, including
two-client synchronization, offline recovery, revocation and cache integrity.
Its host and guest SHA-256 matched. Test processes and exclusively owned temporary
files were cleaned up, and the validation VM was shut down. These focused checks
do not substitute for a full Linux run of the 1,890-test macOS gate.

## Release artifacts

Built with the checked-in nightly toolchain, lockfile and `cargo xtask web-build`.
All three embed browser asset revision
`f0bf29dc0e6e96f0a15aabf7c472afb05d1fdc49fa391bc41842c71bf8ad5aef`.
MiB means 1,048,576 bytes. These measurements include application assets and
dependency notices; they do not measure the compiler or establish a size budget
for arbitrary Fern programs.

| Artifact | Bytes | MiB | Binary validation |
| --- | ---: | ---: | --- |
| `dist/fern-web` — macOS ARM64 | 2,866,992 | 2.73 | Executed; links libSystem and libiconv |
| `dist/fern-web-linux-arm64` | 2,886,680 | 2.75 | Static ELF; executed on ARM64 Linux |
| `dist/fern-web-linux-x86_64` | 3,208,440 | 3.06 | Static ELF; not executed |

SHA-256 values, in the same order:

```text
ccded28d5369732997dbdc5b58c51b5d83a8d95e7a68a35d8c01452178f14cc8
c33896c288da5700556bb5771a58cfeab1291adfa22574acb31755a8e7e2bfd3
20eca6d5dcbe9597339f0f904f4e2fb39c44618b74992c5167dbebb51485b630
```

Both Linux files have no ELF interpreter or required shared libraries. The
ARM64 file also passed the full real-browser suite against actual Linux
execution. Static deployment needs no Fern/Rust installation or asset directory.
Public HTTPS/WSS still requires a TLS terminator.

## Remaining release gates

General numeric/non-tail/loop suspension, precise native allocation layouts,
work stealing, measured sustained throughput/latency under blocking work and
distributed ownership remain open. The application wire schema and build paths
still contain checklist-specific contracts; application-independent packaging
must prove a second structurally different Fern application without host edits.
Durable room state is not an exactly-once transaction for external effects.
WASM closures/maps and x86-64 native execution also need separate acceptance.
