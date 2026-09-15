# Morrow compiler

The `morrow` Cargo package provides the compiler binary and the `morrow_compiler`
library. It uses safe Rust for lexing, parsing, checking, lowering, formatting,
documentation, the REPL and LSP. Cranelift is the native backend. The checked
machine IR also has a textual inspection format exposed by `morrow emit`.

Build the complete workspace from the repository root:

```sh
cargo xtask build
./bin/morrow check examples/tiny_cli.mr
./bin/morrow run examples/tiny_cli.mr
cargo xtask check
```

See [BUILD.md](../../BUILD.md) for the pinned toolchain, host prerequisites,
installation and release packaging. Root `Cargo.lock` fixes compiler dependencies.

## Commands

- `morrow check source.mr`: parse and check an executable module graph.
- `morrow build source.mr -o program`: produce a native executable atomically.
- `morrow run source.mr -- args`: compile in a private directory and forward literal arguments.
- `morrow fmt source.mr`: format source; `--check` reports differences without rewriting.
- `morrow lex source.mr` and `morrow parse source.mr`: inspect tokens or source AST.
- `morrow emit source.mr`: print validated textual machine IR without invoking native tools.
- `morrow repl`: evaluate expressions with persistent bindings and terminal editing.
- `morrow doc source.mr --html`: generate documentation; `--open` opens retained HTML.
- `morrow doc src --site docs-site [--extras docs] [--inferred]`: publish a multi-page
  site with guides, navigation, search and cross-references
  ([guide](../../docs/DOCUMENTATION.md)).
- `morrow test source.mr`: execute zero-argument `test_` functions and documentation examples.
- `morrow lsp`: serve the language-server protocol over standard input/output.

Global `--quiet`, `--verbose` and `--color=auto|always|never` controls preserve
program and protocol output. `run` arguments after `--` remain literal. Help and
version remain visible in quiet mode. Missing commands and invalid options fail.
Use `morrow --help` for the actual accepted CLI surface.

## Language and tooling contracts

The [language guide](../../docs/LANGUAGE_GUIDE.md) introduces functions, immutable
bindings, pattern matching and explicit errors. Detailed contracts cover
[newtypes](../../docs/NEWTYPES.md), [unions](../../docs/UNIONS.md),
[JSON values](../../docs/JSON_RUST_API.md),
[derived codecs](../../docs/JSON_TYPED_CODECS.md),
[actors](../../docs/RUST_ACTORS.md), and
[source tests](../../docs/TEST_RUNNER.md).

LSP includes diagnostics, completion, hover, signatures, navigation, symbols,
formatting, semantic tokens, folding, selection ranges, inlay hints, rename and
code actions. The server uses the compiler's parser and checked module graph.
Configure the executable command `morrow lsp` in a compatible editor; no separately
built editor grammar is distributed.

## Native pipeline and ownership

```text
UTF-8 modules → lexer/parser → checked typed IR → machine IR
             → Cranelift object → host linker + Rust runtime → executable
```

Resolved identities keep source names separate from generated symbols. Semantic
types remain distinct even when their native register widths agree. Native
runtime imports use an audited fixed ABI; the runtime owns allocation, collection
and services. A generated `morrow_main` wrapper preserves the startup calling
convention supplied by the Rust native-entry archive.

`build` checks source/output aliases and publishes only completed artifacts.
Installed compilers resolve the runtime and supervisor beside the actual executable;
the package marker prevents fallback into a development checkout. Explicit
`MORROW_RUNTIME_LIB` and `MORROW_TEST_SUPERVISOR` overrides remain available.

Source tests execute through the Rust retained-child supervisor. Each stream is
limited to 256 KiB, deadlines are positive and at most 60 seconds, and capture
validates a complete binary frame. Native exit 125 remains a child result, distinct
from supervisor transport failure. Child group identity remains owned until
cleanup and reaping; kernel reaping can extend elapsed time. Deliberately escaped
process groups are outside containment and cannot hold capture beyond its deadline.

## Contributing

Run focused tests before broad acceptance:

```sh
cargo test -p morrow --lib
cargo test -p morrow --test lowering_closures
cargo xtask test
cargo xtask check
```

The combined checks build the Rust helper fixtures before compiler integration
tests. Independent native oracles pin output, failures, full-width values and ABI
behavior. Follow [CLAUDE.md](../../CLAUDE.md) and
[MORROW_STYLE.md](../../MORROW_STYLE.md). The [roadmap](../../ROADMAP.md) distinguishes
completed acceptance from planned features. Dated migration/backend reports refer
to their original implementation and do not validate subsequent runtime changes.
