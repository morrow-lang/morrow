# Start writing Morrow

Morrow is a pre-1.0 language for native programs with immutable values, inferred
local types, and explicit errors. This guide uses the working compiler surface.
[DESIGN.md](../DESIGN.md) also describes features still planned; consult
[release readiness](RELEASE_READINESS.md) before choosing Morrow for a project.

## Build and say hello

Follow [BUILD.md](../BUILD.md) to install the build dependencies, then run
`cargo xtask build` in the checkout. Save this program as `hello.mr`:

```morrow
fn main():
    println("Hello, Morrow!")
```
```output
Hello, Morrow!
```

Run it with `./bin/morrow run hello.mr`. To make an executable, use
`./bin/morrow build hello.mr -o hello`, then `./hello`.
`print` writes without a newline; `println` adds one.
The four-space indentation introduces the function body. A `main` without a
return annotation finishes with exit code zero. Use `fn main() -> Int` when you
need to choose a process exit code.

For a local installation, run `cargo xtask install "$HOME/.local"`, then add
`$HOME/.local/bin` to `PATH`. Keep `morrow`, `morrow-test-supervisor`,
`libmorrow_runtime.a` and `morrow-package.json` together when moving an installation.
Compilation still needs the host linker and SDK described in the build guide.

## Values and functions

`let` binds an immutable value. The compiler infers local types; function
signatures make interfaces explicit. The last expression is the return value.

```morrow
fn greet(name: String) -> String:
    String.concat("Hello, ", name)

fn main():
    let language = "Morrow"
    println(greet(language))
    let score = 6 * 7
    println(score)
```
```output
Hello, Morrow
42
```

Use `String`, `List`, `System`, and `Tui.*` for core utilities. Files, HTTP,
SQLite, and actor mailboxes use `fs`, `http`, `sql`, and `actors`. These built-in
modules are available without imports. `File` remains an alias for `fs`.

## Choose a result and work with lists

An `if` is an expression. Both branches produce a value. Lists have one element
type, and list operations return new values. Prefer bounded input sizes while
exploring recursive programs.

```morrow
fn sum(values: List(Int)) -> Int:
    if List.is_empty(values):
        0
    else:
        List.head(values) + sum(List.tail(values))

fn main():
    let scores = [10, 20, 30]
    let extended = List.push(scores, 40)
    println(sum(scores))
    println(sum(extended))
    println(List.len(scores))
```
```output
60
100
3
```

The empty-list check protects `List.head` and `List.tail`. The original `scores`
still contains three elements after constructing `extended`.

## Handle errors explicitly

Fallible library calls return `Result(Value, Int)`: either `Ok(value)` or
`Err(error_code)`. Match both cases. `?` propagates errors inside a function that
itself returns a compatible `Result`.

This example uses an invalid URL so its output is deterministic and it requires
no network access:

```morrow
fn main():
    match http.get("invalid://example"):
        Ok(body) -> println(body)
        Err(_) -> println("Request could not be completed")
```
```output
Request could not be completed
```

`fs.read(path)` returns `Result(String, Int)`. `fs.write(path, content)` returns
`Result(Int, Int)`. HTTP GET and POST return a response body for successful 2xx
responses; transport failures and other statuses return an integer error.
See the [stdlib reference](STDLIB_API_REFERENCE.md) for the current signatures.

## Iterate with the tools

- `morrow check hello.mr` checks syntax and types without linking.
- `morrow fmt hello.mr` formats the source in place.
- `morrow run hello.mr` compiles in a private temporary directory and executes it.
- `morrow build hello.mr -o hello` retains the executable.
- `morrow repl` opens the interactive REPL.
- `morrow lsp` starts the language server for an editor.

Use `morrow --help` for the current command list. Diagnostics include source
locations and hints; fix the earliest error first, then check again. Use
`--color=never` for plain output and `--verbose` to inspect compilation stages.

## Explore working examples

- [Language tour](../examples/language_tour.mr): traits, compile-time constants,
  Sets, JSON round trips and cleanup in one tested program.
- [Tiny CLI](../examples/tiny_cli.mr): command dispatch and string output.
- [Actor mailboxes](../examples/actor_app.mr): enqueue and explicitly receive messages.
- [HTTP errors](../examples/http_api.mr): deterministic client error handling.
- [Terminal project view](../examples/tui_project.mr): tree and log formatting.
- [File operations](../examples/file_io.mr): reads, writes, and Result matching.

`cargo xtask check` typechecks all examples and runs independent native execution
fixtures, including a native and interactive output oracle for the language tour.
The Rust compiler runs [bounded typed native actors](ACTOR_CONTINUATIONS.md),
with [logical cleanup](ACTOR_CLEANUP.md) and an
[interactive virtual-time scheduler](REPL_ACTORS.md). Static traits and explicit
bounds compose with private inference; the [target matrix](LANGUAGE_STATUS.md)
describes the implemented native, REPL and WASM contracts. The
[readiness checklist](RELEASE_READINESS.md) records remaining language and product
work, including broader supervision trees and distributed recovery.
