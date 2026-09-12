> Historical document, retained for architectural context. Its implementation paths, commands and measurements describe the retired setup. See [the current build guide](../../BUILD.md) and [roadmap](../../ROADMAP.md).

# Relocatable Rust preview packages

C remains the shipping compiler and `mise run install` / `release-package` remain
unchanged. This opt-in developer workflow packages an already-built `fern-rs`
with its native helpers. It does not build inputs, install globally, publish a
release, switch the default compiler or open a browser.

The package is usable after moving the complete directory. Checking, emitting
QBE and generating documentation need no Cargo, Python or separately installed
QBE. Native build/run/test require a host C compiler, pkg-config and compatible
BDW GC, SQLite and OpenSSL development libraries. The executables and runtime
archive must already match the declared OS, architecture and native ABI. Naming
an archive `linux-arm64` is metadata, not cross-compilation or ABI validation.
The smoke gate executes the actual chosen inputs on the matching host.

## Explicit immutable inputs

Build the Rust frontend and its matching QBE/runtime/test supervisor through the
ordinary development workflow before packaging. Then select their exact paths;
these tasks deliberately have no build dependencies. For example:

```sh
# Optional preparation when matching release artifacts do not already exist:
mise run rust-release
export FERN_PREVIEW_COMPILER="$PWD/compiler-rs/target/release/fern-rs"
export FERN_PREVIEW_QBE="$PWD/bin/fern-qbe"
export FERN_PREVIEW_SUPERVISOR="$PWD/bin/fern-test-supervisor"
export FERN_PREVIEW_RUNTIME="$PWD/bin/libfern_runtime.a"
export FERN_PREVIEW_VERSION=0.1.0-preview.1
export FERN_PREVIEW_OS=macos         # macos or linux
export FERN_PREVIEW_ARCH=arm64       # arm64 or x86_64
mkdir -p dist
export FERN_PREVIEW_STAGE="$PWD/dist/preview-stage"
export FERN_PREVIEW_ARCHIVE="$PWD/dist/fern-rust-preview.tar.gz"
mise run rust-preview-stage
mise run rust-preview-package
mise run rust-preview-verify
mise run rust-preview-smoke
```

Paths are passed as literal arguments; spaces, Unicode and shell-looking bytes
are supported. A stage must be a new path with an existing parent. Existing
stages, including dangling symlinks, are rejected and preserved. Choose a fresh
stage name for another build. The archive task may atomically replace an existing
regular archive after completely verifying its replacement. Preparation or
verification failures preserve the previous archive's bytes, mode and mtime.

The equivalent direct Python commands are `scripts/package_rust_preview.py
stage`, `package` and `verify`; each command has `--help`. Python is std-only and
used only for developer packaging/testing. `mise run rust-preview-check` runs
its offline regression suite without compiling native components.

## Exact payload and verification

There are exactly seven files, with no directories, symlinks or optional payload:

| File | Mode | Purpose |
| --- | --- | --- |
| `fern-rs` | 0755 | Rust frontend and CLI |
| `fern-qbe` | 0755 | Native backend helper |
| `fern-test-supervisor` | 0755 | Owned native test execution and framing |
| `libfern_runtime.a` | 0644 | Matching native runtime archive |
| `LICENSE` | 0644 | Source project's license |
| `PREVIEW.md` | 0644 | Runtime requirements and usage |
| `fern-rust-preview.json` | 0644 | Format 1 canonical manifest |

The manifest records each of the six other files' byte count, canonical mode and
SHA256, plus the declared version/OS/architecture. The archive contains that
manifest first, then the other files in lexical order, beneath one canonical
`fern-rust-preview-VERSION-OS-ARCH/` prefix. The stage and the internal `extract`
helper expose the seven siblings directly; the CLI verifier never extracts.
Hashes detect corruption, not publisher identity: replacing both a payload and
its manifest is not prevented. Signing, provenance and release publication are
separate work.

Each executable/runtime file is capped at 128 MiB, text/manifest files at 64 KiB,
and the six payloads share a 256 MiB allowance. Compressed input is capped at
256 MiB + 16 KiB + 1 MiB; inflated tar data at 256 MiB + 16 KiB. Reads use opened
regular-file descriptors, reject symlinks and enforce actual read allowances even
if an input grows after its metadata check. Stage copies verify inode, byte count,
mode, timestamps and content hashes, then validate the exact staged bytes before
publication. This is a bounded local workflow, not a transaction against hostile
concurrent filesystem writers or a power-loss durability guarantee.

The verifier accepts only the producer's canonical USTAR header encoding, regular
member type, zero owners/time/padding, exact modes/order/root and one fixed-header
gzip member with a valid checksum/trailer. It rejects tar extensions, duplicate or
extra paths, absolute/traversal names, missing entries, devices/links, malformed
JSON/duplicate keys, oversized metadata/payloads, concatenated gzip members,
truncation and trailing bytes. Inflation streams to bounded private scratch
storage; no attacker-controlled path is extracted. The test-only extraction API
fully verifies before copying fixed names into a new owned directory.

Identical input bytes and metadata produce identical archive bytes under the same
packager/zlib toolchain, independent of source mtimes, source read permissions,
uid/gid, output name and directory. This is packaging reproducibility, not a claim
of reproducible Rust/C compilation or byte identity across zlib versions.

## Component selection and moved-package proof

The sibling manifest is also a preview marker. If any required sibling is
missing, its presence disables implicit lookup in the compiler's original source
checkout. Empty, malformed, nonregular and dangling markers do not re-enable
that fallback. Existing development binaries without the marker retain their
normal checkout lookup. Explicit `FERN_QBE`, `FERN_RUNTIME_LIB` and
`FERN_TEST_SUPERVISOR` overrides remain supported and take precedence.

The smoke gate stages and archives actual inputs, verifies/extracts them into a
literal Unicode path, removes the original stage and clears the component
overrides. It exercises check/emit/build/run, Unit and Result test entries and
HTML documentation, rejects each missing helper, and tests the three explicit
overrides. PATH traps make any attempted Cargo/Python/standalone-QBE invocation
fail. It compares all immutable input and moved payload hashes afterward. It
never launches a browser or installs a package. Full macOS/Linux proof belongs to
the matching native platform gates; one host's success is not the other host's
verification or a default compiler migration.
