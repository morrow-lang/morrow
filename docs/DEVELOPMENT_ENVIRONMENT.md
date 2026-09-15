# Development environment

Use the Cargo workspace at the repository root. `rust-toolchain.toml` selects
nightly-2026-09-06 with rustfmt and Clippy. `mise.toml` provides the same toolchain
and aliases; it is optional when rustup supplies the pinned compiler.

```sh
cargo xtask build
cargo xtask check
cargo xtask fuzz 512 0xC0FFEE
mise run check
```

`cargo xtask check` checks formatting, all-target Clippy, workspace tests, native
output fixtures, examples and deterministic fuzzing. Compiler unit/doc tests and
the terminal/LSP process tests run as ordinary Cargo tests. Runtime and supervisor
crates have separate focused test targets. Build before directly running tests
that link the native runtime archive or invoke the supervisor.

Cargo.lock fixes production and tooling dependencies. The independent Criterion
workspace under `benchmarks/compiler-phases` has its own lock and documented
commands. Cargo owns incremental artifacts under target/; bin/ contains the
complete locally staged native compiler. No Python, Node, QBE or legacy bootstrap
compiler is part of the development environment.

The host linker and SDK remain necessary for native executables and Rust-wrapped
third-party SQLite/crypto dependencies. See [BUILD.md](../BUILD.md) for supported
platforms, installation and troubleshooting.

## Editor integration

Configure your editor's LSP client with language ID `morrow`, source file pattern
`**/*.mr`, and server command `morrow lsp` (or `./bin/morrow lsp` for a local
build). The compiler's own parser provides language support; no separate editor
grammar or parser toolchain is required by the server.
