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
The same typed application can run as native server actors and as a reactive
WebAssembly browser client, with WebSocket updates and offline continuity.

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
[what remains](docs/RELEASE_READINESS.md), or compare the
[implemented language across targets](docs/LANGUAGE_STATUS.md).

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
- **A growing full-stack path.** Compile Fern functions to WebAssembly and try
  a collaborative checklist with local browser interaction, WebSocket updates
  and cached offline viewing. Its Rust server can ship as one static Linux binary.

The direction is a practical language for command-line tools and applications:
readable code, predictable behavior and a useful standard library. The
[design](DESIGN.md) describes that larger vision; the [roadmap](ROADMAP.md)
tracks its implementation. The full-stack direction combines supervised native
actors with a reactive Fern WebAssembly client over typed WebSocket connections.
The [working web preview](docs/WEB_PREVIEW.md) proves the first integration;
the [architecture](docs/FULL_STACK_ARCHITECTURE.md) defines the remaining
language, supervision and scaling work.

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
./bin/fern run examples/language_tour.fn
```

The example prints `hello, fern`. `run` compiles and executes in one step;
`build` leaves an executable you can run directly.
The [language tour](examples/language_tour.fn) combines traits, compile-time
constants, immutable sets, typed JSON and deferred cleanup in one small program.

The repository selects its pinned Rust nightly automatically through
`rust-toolchain.toml`; Cargo uses the checked-in dependency lock. Mise is optional.
See the [build guide](BUILD.md) for prerequisites and platform details.

## Try the web preview

With the Rust prerequisites above, install the browser build tools once:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
cargo xtask web-build
FERN_WEB_ACCESS_KEY='replace-with-a-long-random-secret' ./dist/fern-web
```

Open **http://127.0.0.1:3000**, sign in with your chosen key and try two browser
windows. Add tasks, toggle completion and filter locally. After the first online
visit has cached the application, offline reload preserves the last confirmed
list and your draft. Shared changes require a connection.

The binary embeds the HTML, CSS, browser WASM, generated bindings and Rust service
worker. [The shared Fern application](examples/web/checklist.fn) owns the domain
model, local model, event updates and keyed view. Its [native room actor](examples/web/server.fn)
owns authoritative state. Rust provides the DOM, storage, authenticated transport
and a rooted native embedding boundary; the shipped server contains no compiler
or interpreter.

Set `FERN_WEB_DATA_DIR=./fern-data` to checkpoint acknowledged room changes and
recover them after a restart. Authentication and command namespaces start fresh;
uncertain commands are never blindly replayed into a new incarnation.

Rooms run on independent actor worker threads: by default, up to four available
CPU cores. Set `FERN_WEB_WORKERS=1` through `32` to choose the worker count.
Authentication stays responsive while a room executes, and admission limits
remain shared across workers. See the [worker contract](docs/WEB_WORKERS.md)
for room placement, revocation and durable-write behavior.

**Connect multiple servers:** `fern-web --cluster-init` creates private node
bundles; `FERN_WEB_CLUSTER` enables authenticated TLS routing to fixed room
owners. Browsers keep their normal automatic WebSocket connection to a gateway.
The [three-node guide](docs/CLUSTER.md) includes setup, delivery guarantees and
the 10,000-mutation stress scenario. Membership is fixed; partitions do not
trigger ownership takeover or replay uncertain mutations.

The [protocol comparison](docs/NETWORK_PROTOCOL.md) explains the WebSocket/JSON
default and measures CBOR/protobuf alternatives without adding them to production.

See the [web guide](docs/WEB_PREVIEW.md) for Linux static builds, authentication,
offline behavior and the exact preview boundary.

Open **`/admin`** after signing in to inspect uptime, resident memory, workers, rooms,
connections and resource limits. The [system dashboard](docs/ADMIN_DASHBOARD.md)
also provides authenticated JSON snapshots and configured versus connected peers.

## Explore resilience

Run the actual protocol and native Fern actors under reproducible faults and
virtual time, then replay the result:

```sh
cargo xtask simulate --seed 42 --steps 3000 --days 30 --json > scenario.json
cargo xtask simulate --replay scenario.json
cargo xtask simulate --actors --seed 42 --steps 5000
```

The [demo and simulation guide](docs/DETERMINISTIC_SIMULATION.md) combines native
supervision, the offline WASM draft preview and deterministic failure testing.
Simulated time measures the scenario's clock; it is not a production-uptime claim.

## Available today

- Immutable bindings, inferred types and functions that return their last expression.
- Integers, floating-point values, strings, collections, records,
  tagged sums, newtypes and finite unions.
- Exhaustive pattern matching, `Option`, `Result`, `?` and checked error handling.
- Modules, closures, generic functions and [static traits](docs/TRAITS.md) with
  defaults, explicit bounds and structural derivation.
- Derived and [custom JSON codecs](docs/CUSTOM_JSON.md), including nested union
  discrimination, precise error paths and shared resource limits.
- Immutable [Sets](docs/SETS.md), [compile-time constants](docs/COMPTIME.md), and
  explicit [native foreign functions](docs/FFI.md) with checked ABI types.
- Native services for files, processes, HTTP clients, SQLite, terminal widgets
  and bounded actor execution with [suspended cleanup](docs/ACTOR_CLEANUP.md).
- [Interactive actors and deterministic replay](docs/REPL_ACTORS.md) for testing
  mailboxes, timeouts, restarts and cancellation without real-time sleeps.

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
them. A Rust-owned tracing collector manages native Fern values, with isolated
actor payload heaps and copied messages. Compiler root frames are explicit;
conservative scanning remains while precise-root coverage is completed.
Ordinary Fern code does not require borrow checking or lifetime annotations.

We prefer native Rust dependencies and permit maintained Rust wrappers around
third-party native libraries. SQLite uses `rusqlite`; the native HTTP client uses `ureq` and
`rustls`. A Rust implementation does not imply that every transitive dependency
is Rust. [Dependency notices](THIRD_PARTY_NOTICES.md) document the shipped libraries.

Editor support uses the Rust LSP and the compiler's parser. Tree-sitter integration
has been removed. Cargo and `xtask` own the build, checks and distribution workflow.
Browser interop JavaScript is generated during the build; the browser host,
service worker and build pipeline are authored in Rust.

## Status and next steps

The [2026-09-12 Rust migration acceptance record](docs/RUST_WORKSPACE.md) covers both ARM64 hosts:
over 1,750 Rust tests per platform, 305 native-output fixtures, compatibility and
fuzz checks, optimized runtime/ABI tests, and real archive relocation,
installation and source-test execution. Formatting and Clippy are part of the
required quality gate.
That record predates the new actor-heap and web work. Fresh macOS ARM64 checks
passed; Linux ARM64 completed equivalent coverage across resumed runs after a
storage interruption. The [web guide](docs/WEB_PREVIEW.md#verification) records
the exact native scope and real-browser checks, including offline worker restart,
cache integrity, mobile layout and static ARM64 server execution.
The [application and worker acceptance record](docs/WEB_APPLICATION_ACCEPTANCE.md)
adds the complete 1,890-test gate, actor lifecycle/progress checks and measured
static servers of 2.75 MiB (ARM64) and 3.06 MiB (x86-64).

Fern is ready to explore, build small programs with and contribute to. It remains
an early preview: general actor preemption, generalized supervision, work
stealing, dynamic cluster membership, replicated failover, remote language PIDs
and a general application packaging API remain open.
The current checklist executes its complete typed model/update/view and native
actor path, with optional durable room checkpoints. WASM supports bounded
records, tagged sums, lists, tuples, Option/Result, UTF-8 strings, closures,
maps, sets, ranges, iteration, deferred cleanup and structural unions with precise
tracing and rooted host handles; see [portable language support](docs/WASM_LANGUAGE.md).
Native services remain separate capabilities. [Static traits](docs/TRAITS.md),
[custom JSON codecs](docs/CUSTOM_JSON.md), compile-time constants and Sets work
through the checked language pipeline. Broader SQL APIs, x86-64 native-language
acceptance and source debugging remain open. The REPL supports a smaller host
service surface than native programs.
The Rust rewrite is complete;
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
| [`crates/fern-web-protocol`](crates/fern-web-protocol) | Authenticated command/revision contracts and application transitions |
| [`crates/fern-web-app`](crates/fern-web-app) | Compiled Fern actor embedding and durable room checkpoints |
| [`crates/fern-web`](crates/fern-web) | Authenticated HTTP/WebSocket preview server and embedded assets |
| [`crates/fern-browser`](crates/fern-browser) | Rust browser host, generic keyed DOM and Fern model handles |
| [`crates/fern-browser-worker`](crates/fern-browser-worker) | Rust service worker for cached offline loading |
| [`crates/fern-test-supervisor`](crates/fern-test-supervisor) | Native test capture and process cleanup |
| [`xtask`](xtask) | Build, checks, packaging and installation |

The [contribution guide](CLAUDE.md) requires tests before behavior changes.
See [FERN_STYLE.md](FERN_STYLE.md) for Rust safety and resource-bound conventions.
The [documentation index](docs/README.md) connects language references, contracts
and acceptance reports.

Released under the [MIT License](LICENSE).
