<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/fern-logo-light.png">
    <img src="docs/assets/fern-logo.png" alt="Fern logo" width="128" height="128">
  </picture>
</p>

<h1 align="center">Fern</h1>

<p align="center"><strong>Readable code. Native programs.</strong><br>
A statically typed, functional language with Python-like syntax.</p>

Fern brings immutable values, pattern matching and explicit errors to readable,
indentation-based code. Write Fern, check it before execution, and compile it to a
native executable with a Rust compiler and runtime.

```fern
fn greet(name: String) -> String:
    "Hello, {name}!"

fn main():
    println(greet("Fern"))
```

**Early preview, with a working Rust implementation.** The compiler, runtime,
language server and repository tools are Rust. Native execution and release
installation are verified on **macOS ARM64 and Linux ARM64**. Syntax and APIs
are still evolving; see [what is verified](docs/RUST_WORKSPACE.md) and
[what remains](docs/RELEASE_READINESS.md).

## Why Fern?

- **Code that reads clearly.** Significant indentation, inferred types, immutable
  bindings and expressions that return values keep everyday programs direct.
- **Errors you can see.** `Option`, `Result` and exhaustive pattern matching make
  absence and failure explicit. The compiler checks that results are handled.
- **Useful tools together.** A formatter, REPL, source tests, documentation
  generator and language server ship with the compiler.
- **Native programs.** Cranelift generates machine code and the Rust runtime
  supplies memory management and services. Running a compiled program does not
  require the Fern compiler; platform system libraries still apply.

The direction is a practical language for command-line tools and applications:
readable code, predictable behavior and a useful standard library. The
[design](DESIGN.md) describes that larger vision; the [roadmap](ROADMAP.md)
tracks its implementation.

## Try it

You need **rustup** and a **host compiler/linker**: Xcode command-line tools on
macOS, or GCC/Clang with the platform development libraries on Linux.

```sh
git clone https://github.com/niklas-heer/fern.git
cd fern
cargo xtask build --release
./bin/fern run examples/tiny_cli.fn
./bin/fern build examples/tiny_cli.fn -o hello
./hello
```

The example prints `hello, fern`. `run` compiles and executes in one step;
`build` leaves an executable you can run directly.

The repository selects its pinned Rust nightly automatically through
`rust-toolchain.toml`; Cargo uses the checked-in dependency lock. Mise is optional.
See the [build guide](BUILD.md) for prerequisites and platform details.

## Available today

- Immutable bindings, inferred types and functions that return their last expression.
- Integers, floating-point values, strings, collections, records,
  tagged sums, newtypes and finite unions.
- Exhaustive pattern matching, `Option`, `Result`, `?` and checked error handling.
- Modules, closures, generic functions and derived JSON codecs.
- Native services for files, processes, HTTP clients, SQLite, terminal widgets
  and bounded actor execution.

Follow the [language guide](docs/LANGUAGE_GUIDE.md), explore
[examples](examples), or read the [standard-library reference](docs/STDLIB_API_REFERENCE.md).

## Compiler tools

```sh
./bin/fern check source.fn
./bin/fern fmt source.fn
./bin/fern test source.fn
./bin/fern doc source.fn --html
./bin/fern repl
./bin/fern lsp
```

Replace `source.fn` with your program. The language server provides diagnostics,
completion, navigation, formatting, rename and more over standard input/output.
Configure your editor to launch `fern lsp` using the installed executable or its
absolute path. See the [compiler guide](crates/fern/README.md) for command details
and the inspection tools.

## Our implementation stance

Fern-owned implementation and development tooling stay in **Rust**, organized as
a Cargo workspace. We prefer safe ownership, explicit resource limits and small,
documented unsafe boundaries where native ABI and operating-system access require
them. A Rust-owned tracing collector manages native Fern values.

We prefer native Rust dependencies and permit maintained Rust wrappers around
third-party native libraries. SQLite uses `rusqlite`; HTTP uses `ureq` and
`rustls`. A Rust implementation does not imply that every transitive dependency
is Rust. [Dependency notices](THIRD_PARTY_NOTICES.md) document the shipped libraries.

Editor support uses the Rust LSP and the compiler's parser. Tree-sitter integration
has been removed. Cargo and `xtask` own the build, checks and distribution workflow.

## Status and next steps

The [2026-09-12 acceptance record](docs/RUST_WORKSPACE.md) covers both ARM64 hosts:
over 1,750 Rust tests per platform, 305 native-output fixtures, compatibility and
fuzz checks, optimized runtime/ABI tests, and real archive relocation,
installation and source-test execution. Formatting and Clippy are part of the
required quality gate.

Fern is ready to explore, build small programs with and contribute to. It remains
an early preview: generalized actor supervision, custom traits, HTTP serving,
broader SQL APIs, advanced ownership analysis and a WASM backend are still planned.
x86-64 acceptance and source debugger support also remain open. The REPL supports
a smaller execution surface than native programs. The Rust rewrite is complete;
the [language roadmap](ROADMAP.md) continues.

## Install or contribute

```sh
cargo xtask install "$HOME/.local"
```

Add `$HOME/.local/bin` to your `PATH` to use `fern` directly. Installation builds
release components. To remove them, run `cargo xtask uninstall "$HOME/.local"`.

For development and packaging:

```sh
cargo xtask check
cargo xtask package
```

`check` builds the components and runs formatting, linting, Rust tests and native
acceptance. `package` creates a verified host archive and checksum in `dist/`.
The release bundle includes `fern`, the Rust test supervisor, the Rust runtime
archive, its package marker and license notices. Keep these components together
when moving an installation.

| Directory | Responsibility |
| --- | --- |
| [`crates/fern`](crates/fern) | Compiler, CLI, formatter, REPL, docs and LSP |
| [`crates/fern-runtime`](crates/fern-runtime) | Native values, collector and services |
| [`crates/fern-runtime-native`](crates/fern-runtime-native) | Compiled-program startup |
| [`crates/fern-json`](crates/fern-json) | Shared bounded JSON implementation |
| [`crates/fern-test-supervisor`](crates/fern-test-supervisor) | Native test capture and process cleanup |
| [`xtask`](xtask) | Build, checks, packaging and installation |

The [contribution guide](CLAUDE.md) requires tests before behavior changes.
See [FERN_STYLE.md](FERN_STYLE.md) for Rust safety and resource-bound conventions.
The [documentation index](docs/README.md) connects language references, contracts
and acceptance reports.

Released under the [MIT License](LICENSE).
