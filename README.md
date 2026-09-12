<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/fern-logo-light.png">
    <img src="docs/assets/fern-logo.png" alt="Fern logo" width="128" height="128">
  </picture>
</p>

<h1 align="center">Fern</h1>

<p align="center"><strong>Readable code. Native programs.</strong><br>
A statically typed, functional language with Python-like syntax.</p>

Fern combines immutable values, pattern matching and explicit errors with
indentation-based syntax. Its compiler, runtime, language server and development
tools are implemented in Rust. Cranelift compiles checked programs to native objects.

```fern
fn greet(name: String) -> String:
    "Hello, {name}!"

fn main():
    println(greet("Fern"))
```

**Early preview.** Syntax and APIs are evolving. [ROADMAP.md](ROADMAP.md) records
verified work and remaining limitations; [DESIGN.md](DESIGN.md) also describes
planned features.

## Try it

Install Rust through rustup and a host compiler/linker, then:

```sh
git clone https://github.com/niklas-heer/fern.git
cd fern
cargo xtask build
./bin/fern run examples/tiny_cli.fn
./bin/fern build examples/tiny_cli.fn -o hello
./hello
```

The root toolchain file selects the pinned Rust nightly automatically.
See [BUILD.md](BUILD.md) for prerequisites, testing and installation.

## The language

- Immutable bindings, inferred types and functions that return their last expression.
- Full-width integers, floating-point values, strings, collections, records,
  tagged sums, newtypes and finite unions.
- Exhaustive pattern matching, `Option`, `Result`, `?` and checked error handling.
- Modules, closures, generic functions and derived JSON codecs.
- Native services for files, processes, HTTP clients, SQLite, terminal output
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

The language server speaks LSP over standard input/output and can be configured
in any compatible editor. The compiler also provides `lex`, `parse` and textual
machine-IR inspection through `emit`. See the [compiler guide](crates/fern/README.md).

## Develop and install

```sh
cargo xtask check
cargo xtask build --release
cargo xtask package
cargo xtask install "$HOME/.local"
```

`check` runs formatting, linting, Rust tests and native behavior checks.
The release bundle includes `fern`, the Rust test supervisor, the Rust runtime
archive, its package marker and license notices. Keep these components together
when moving an installation. Cargo builds bundled third-party native dependencies
where needed; Fern-owned implementation code remains Rust.

The [contribution guide](CLAUDE.md) requires tests before behavior changes.
See [FERN_STYLE.md](FERN_STYLE.md) for Rust safety and resource-bound conventions.
Historical reports document the implementation that existed when measured; they
are not acceptance evidence for subsequent backend or runtime changes.

Released under the [MIT License](LICENSE).
