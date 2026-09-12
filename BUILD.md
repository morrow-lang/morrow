# Build and develop Fern

Fern is a Cargo workspace. The Rust compiler uses Cranelift for native code,
and links generated objects with the Rust runtime archive using the host linker.

## Prerequisites

- Rustup, with the toolchain selected by `rust-toolchain.toml`:
  **nightly-2026-09-06**, including rustfmt, Clippy and rust-src.
- Linux or macOS on a supported 64-bit host.
- A host C compiler/linker and platform SDK. Linux build-essential or Clang and
  the macOS Xcode command-line tools provide these.

Cargo builds SQLite through `rusqlite`'s bundled feature and the TLS provider
through its Rust wrapper. These third-party dependencies can compile native code.
Separate SQLite or OpenSSL development packages are not required. Runtime and
compiler dependencies are locked in root `Cargo.lock`.

Mise is optional; its configuration selects the same Rust toolchain. Direct Cargo
commands use the root toolchain file. The numeric `rust-version` is Cargo's minimum
version check, not a promise of support for an untested stable compiler.

## Build and run

```sh
cargo xtask build
./bin/fern run examples/tiny_cli.fn
./bin/fern build examples/tiny_cli.fn -o hello
./hello
cargo xtask build --release
```

The staged `bin/` directory contains `fern`, `fern-test-supervisor`,
`libfern_runtime.a` and `fern-package.json`. The archive includes native startup
and the Rust runtime. Cargo's separate core runtime archive is for ABI/collector
probes and is not a substitute for the staged startup archive.

## Checks

| Command | Purpose |
| --- | --- |
| `cargo xtask check` | Formatting, Clippy, workspace tests and native acceptance |
| `cargo xtask test` | Workspace tests and native acceptance |
| `cargo xtask native` | Execute expected-output native fixtures using staged binaries |
| `cargo xtask examples` | Typecheck public examples using staged binaries |
| `cargo xtask fmt` | Format the Rust workspace |
| `cargo xtask lint` | Check formatting and deny Clippy warnings |
| `cargo test -p fern --lib` | Focused compiler library tests |
| `cargo test -p fern-runtime` | Runtime unit tests |

Build before running `native` or `examples` directly. The combined `check` and
`test` commands prepare their own binaries and Rust supervisor fixtures. Run
focused tests first while developing, then the complete check before committing.

The workspace disables incremental compilation and debug symbols in development
and test profiles to bound artifacts from the large test suite. Keep the Cargo
target and temporary directories on a disk with adequate free space. Use
`cargo clean` only when a deliberate generated-artifact reset is needed.

## Install and relocate

```sh
cargo xtask install "$HOME/.local"
cargo xtask uninstall "$HOME/.local"
```

Installation builds release components. Executables, the startup archive and
marker live under `<prefix>/bin`; license notices and the optional README live
under `<prefix>/share/fern`. Uninstall removes only these known component names.
Literal spaces, quotes and Unicode are supported. Installation preflights every
destination, prepares complete private copies, and atomically replaces each file.
It rejects symlink components in the destination path; use the actual directory
path when a system alias such as macOS `/tmp` resolves through a symlink.

The compiler locates native components beside its actual executable, including
when invoked through `PATH` or a symlink. An installed package marker prevents
fallback to a development checkout. `FERN_RUNTIME_LIB` and
`FERN_TEST_SUPERVISOR` are explicit component overrides; `CC` selects one linker
driver executable and does not accept a shell command.

## Release archives

```sh
cargo xtask package
cargo xtask package /absolute/output/directory
cargo xtask verify dist/fern-0.1.0-linux-arm64.tar.gz dist/fern-0.1.0-linux-arm64.tar.gz.sha256
```

Archive names include the workspace version and actual host OS/architecture.
Verification checks the SHA-256 record, complete tar/gzip framing, exact component
inventory, marker, regular-file types, permissions and byte limits without
extracting files. Packaging verifies private output before publication. Generated
programs still depend on platform system libraries; bundles are platform-specific.

## Workspace layout

- `crates/fern`: compiler, formatter, REPL, docs, LSP and language tests.
- `crates/fern-runtime`: allocation, native value ABI and services.
- `crates/fern-runtime-native`: compiled-program startup archive.
- `crates/fern-json`: shared bounded JSON implementation.
- `crates/fern-test-supervisor`: retained-child native test capture and protocol.
- `xtask`: build, acceptance, distribution and installation commands.
- `examples` and `docs`: language examples and reference material.

See [CLAUDE.md](CLAUDE.md), [FERN_STYLE.md](FERN_STYLE.md) and
[ROADMAP.md](ROADMAP.md) before making changes. Dated reports describe their
original implementation and must not be relabeled as measurements of a new build.
