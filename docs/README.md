# Morrow documentation

Start with the [language guide](LANGUAGE_GUIDE.md), [build guide](../BUILD.md), and
[current roadmap](../ROADMAP.md). The implementation is a Rust workspace; the
[design](../DESIGN.md) also records features that remain planned.

- [Standard library](STDLIB_API_REFERENCE.md)
- [Result handling](RESULT_HANDLING.md), [labeled calls](LABELED_CALLS.md), [newtypes](NEWTYPES.md), [unions](UNIONS.md)
- [Native actors](RUST_ACTORS.md) and [mailbox/supervision API](ACTOR_RUNTIME.md)
- [JSON values](JSON_RUST_API.md) and [typed codecs](JSON_TYPED_CODECS.md)
- [File text IO](FILE_TEXT_IO.md), [process execution](PROCESS_EXECUTION.md), [SQL lifecycle](SQL_LIFECYCLE.md)
- [Runtime memory](MEMORY_MANAGEMENT.md) and [Unicode decimal classification](STRING_DECIMAL.md)
- [Writing and publishing documentation](DOCUMENTATION.md) with `@moduledoc`, `@doc` and `morrow doc --site`
- [Development environment](DEVELOPMENT_ENVIRONMENT.md), [test runner](TEST_RUNNER.md), [Rust style](../MORROW_STYLE.md)
- [Rust workspace acceptance](RUST_WORKSPACE.md)
- [Full-stack actor/WebAssembly architecture](FULL_STACK_ARCHITECTURE.md)
- [Browser preview: WebSockets, offline reload and standalone deployment](WEB_PREVIEW.md)
- [System dashboard and authenticated runtime snapshots](ADMIN_DASHBOARD.md)
- [Compatibility policy](COMPATIBILITY_POLICY.md) and [release readiness](RELEASE_READINESS.md)

Editor support is the Rust language server (`morrow lsp`). Tree-sitter and the
associated Zed grammar package were removed as part of the Rust-only migration.
Source documentation uses `morrow doc`; `cargo xtask docs` renders these guides,
the examples and the Rust API reference (`cargo doc --workspace --no-deps`) into
one browsable site under `dist/docs`.

The [history](HISTORY.md), [archived migration documents](history/) and [reports](reports/)
retain dated decisions and measurements. Their old C/QBE/Python commands are
historical evidence, not the current development workflow.
