<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/morrow-lang/morrow/main/docs/assets/morrow-logo-dark.svg">
    <img src="https://raw.githubusercontent.com/morrow-lang/morrow/main/docs/assets/morrow-logo.svg" alt="Morrow logo" width="128" height="128">
  </picture>
</p>

<h1 align="center">Morrow</h1>

<p align="center"><strong>Readable code. Native programs. Isolated actors.</strong><br>
A statically typed, functional language with Python-like syntax.</p>

<p align="center">
  <a href="https://morrow-lang.org">Project home</a> ·
  <a href="docs/README.md">Documentation</a> ·
  <a href="docs/LANGUAGE_GUIDE.md">Language guide</a> ·
  <a href="ROADMAP.md">Roadmap</a>
</p>

```morrow
fn greet(name: String) -> String:
    "Hello, {name}!"

fn main():
    println(greet("Morrow"))
```

Morrow combines things that usually live in different languages: Python-like
syntax, Gleam-style type safety, Go-style single binaries and Erlang-style
isolated processes. The compiler, runtime, language server and tooling are Rust;
Cranelift generates machine code, so there is no virtual machine to ship. The same
typed program can run as native server actors and as a reactive WebAssembly
browser client.

> **Early preview.** Native execution and release installation are verified on
> macOS ARM64 and Linux ARM64. Syntax and APIs are still evolving. Native
> sequential code is already fast; matching the BEAM's actor throughput and
> OTP-style supervision is the open hard problem, and the
> [measurements](benchmarks/language-comparison/ACTORS.md) say so.

## Why Morrow

- **Code that reads clearly.** Significant indentation, inferred types,
  immutable bindings and expressions that return values.
- **Errors you can see.** `Option`, `Result`, `?` and exhaustive pattern
  matching. The compiler checks that results are handled.
- **Native programs.** Machine code plus a Rust runtime for memory management
  and services. Running a compiled program does not require the compiler.
- **Isolated actors without a VM.** Every actor owns its heap; messages are
  copied, never shared. Multiple scheduler threads, cooperative preemption,
  optional work stealing, and typed [monitors and links](docs/PROCESS_MODEL.md).
- **Batteries included.** Formatter, REPL, source tests, documentation generator
  and language server ship with the compiler.
- **A full-stack path.** Compile Morrow to WebAssembly and run the same typed
  model in the browser, connected to native actors over WebSockets, with
  offline continuity. The [web preview](docs/WEB_PREVIEW.md) shows it working.

## Try it

You need **rustup** and a host compiler/linker (Xcode command-line tools on macOS,
GCC/Clang with platform development libraries on Linux). The pinned Rust nightly
and dependency lock are selected automatically.

```sh
git clone https://github.com/morrow-lang/morrow.git
cd morrow
cargo xtask build --release
./bin/morrow run examples/tiny_cli.mr          # prints: hello, morrow
./bin/morrow build examples/tiny_cli.mr -o hello && ./hello
./bin/morrow run examples/language_tour.mr
```

To install `morrow` into `~/.local/bin`:

```sh
cargo xtask install "$HOME/.local"
```

More in the [build guide](BUILD.md). The [language tour](examples/language_tour.mr)
shows traits, compile-time constants, immutable sets, typed JSON and deferred
cleanup in one small program; the [examples](examples) directory has more.

## What works today

- Immutable bindings, inferred types, closures, modules and generic functions.
- Integers, floats, strings, collections, records, tagged sums, newtypes and
  finite unions, with exhaustive matching.
- [Static traits](docs/TRAITS.md) with defaults, bounds and derivation;
  derived and [custom JSON codecs](docs/CUSTOM_JSON.md); immutable
  [sets](docs/SETS.md); [compile-time constants](docs/COMPTIME.md); checked
  [native FFI](docs/FFI.md).
- Native services for files, processes, HTTP clients, SQLite and terminal
  widgets; see the [standard library reference](docs/STDLIB_API_REFERENCE.md).
- Isolated native actors with copied messages, typed `Process` monitors, links
  and exit signals, [suspended cleanup](docs/ACTOR_CLEANUP.md) and
  [deterministic replay in the REPL](docs/REPL_ACTORS.md).

Compare the implemented language across native, REPL and WebAssembly targets in
[language status](docs/LANGUAGE_STATUS.md).

## Tools

```sh
morrow check source.mr        # typecheck
morrow fmt source.mr          # format
morrow test source.mr         # run source tests and doc examples
morrow doc src --site site    # searchable documentation site
morrow repl                   # interactive session
morrow lsp                    # language server over stdio
```

Documentation lives next to the code in `@moduledoc` and `@doc`, and its examples
run as tests. See the [compiler guide](crates/morrow/README.md) and
[writing documentation](docs/DOCUMENTATION.md).

## The web preview

A collaborative checklist whose domain model, local updates and keyed view are
one shared Morrow program compiled to WebAssembly, with a native room actor owning
authoritative state. The server is a single static Linux binary that embeds the
assets and a Rust service worker for offline reload.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
cargo xtask web-build
./dist/morrow-web
```

Open <http://127.0.0.1:3000> in two windows. The header shows how many browsers
are in the room. Publishing this demo to Fly.io and the documentation site is
in the [deployment guide](docs/DEPLOY.md). The [web guide](docs/WEB_PREVIEW.md)
covers durable checkpoints, worker threads, the
[three-node cluster](docs/CLUSTER.md), the [protobuf wire protocol](docs/NETWORK_PROTOCOL.md)
and the [system dashboard](docs/ADMIN_DASHBOARD.md). Reproducible fault injection
under virtual time is in the [simulation guide](docs/DETERMINISTIC_SIMULATION.md).

## Performance, honestly

- **Sequential native code** is within roughly 16% of optimized Rust on the
  measured arithmetic workload and about 3× faster than Elixir on immutable
  updates. See [arithmetic](benchmarks/language-comparison/ARITHMETIC.md) and
  [BEAM](benchmarks/language-comparison/BEAM.md) comparisons.
- **Actors** are measured against Elixir/OTP at one, two and four schedulers.
  The latest checkpoint passes zero of nine strict parity cells; one-scheduler
  contention reaches 0.785× BEAM and request/reply 0.304×. See the
  [actor comparison](benchmarks/language-comparison/ACTORS.md).

These are specific whole-process experiments, not a language ranking. The
[benchmark guide](benchmarks/README.md) has the commands to rerun them.

## Status

The Rust implementation is complete within its
[recorded acceptance](docs/RUST_WORKSPACE.md): thousands of Rust tests, hundreds of
native-output fixtures, fuzzing, and real archive installation on both ARM64
hosts. Formatting and Clippy are part of the required gate.

Still open: OTP-style supervisor trees, BEAM actor-throughput parity, dynamic
cluster membership and replicated failover, x86-64 native acceptance, source
debugging and broader SQL and HTTP-serving APIs. The [roadmap](ROADMAP.md) is the
dated record of what is verified; [release readiness](docs/RELEASE_READINESS.md)
lists what remains.

## Contributing

```sh
cargo xtask check      # build, format check, Clippy, Rust tests, native acceptance
cargo xtask package    # verified host archive in dist/
```

Morrow-owned code is Rust: safe ownership, explicit resource limits and small,
documented unsafe boundaries at the native ABI and OS edges. Behavior changes
need a failing test first; see the [contributor guide](CLAUDE.md) and
[style guide](MORROW_STYLE.md).

| Directory | Responsibility |
| --- | --- |
| [`crates/morrow`](crates/morrow) | Compiler, CLI, formatter, REPL, docs and LSP |
| [`crates/morrow-runtime`](crates/morrow-runtime) | Native values, collector, actors and services |
| [`crates/morrow-web*`](crates) | Web protocol, application host and preview server |
| [`crates/morrow-browser*`](crates) | Rust browser host and offline service worker |
| [`xtask`](xtask) | Build, checks, packaging and installation |

Released under the [MIT License](LICENSE).
