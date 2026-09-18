# Getting started

This chapter installs the compiler, runs a first program and explains how a Morrow
program is laid out: the entry point, exit codes, comments, and how source files
form modules.

## Installing

Morrow is built from the repository with Cargo. The
[build guide](../../BUILD.md) lists the prerequisites: a Rust toolchain matching
`rust-toolchain.toml`, and a host C compiler and linker (Xcode command-line tools
on macOS, `build-essential` or Clang on Linux). Native executables are linked
with the host linker, so the linker stays a requirement after installation.

```sh
git clone https://github.com/morrow-lang/morrow
cd morrow
cargo xtask build
```

`cargo xtask build` stages the compiler and its companions in `bin/`: `morrow`,
`morrow-test-supervisor`, the runtime archive `libmorrow_runtime.a` and
`morrow-package.json`. Run the compiler from there while working inside the
checkout:

```sh
./bin/morrow run examples/hello.mr
```

To install a release build for everyday use, pass a prefix. The executables land
in `<prefix>/bin`:

```sh
cargo xtask install "$HOME/.local"
export PATH="$HOME/.local/bin:$PATH"
morrow --help
```

The four staged components locate each other beside the `morrow` executable, so
move them together if you relocate an installation. `cargo xtask uninstall` with
the same prefix removes them again.

## Hello, Morrow

Save the following as `hello.mr`:

```morrow
fn main():
    println("Hello, Morrow!")
```

Run it:

```sh
morrow run hello.mr
```
```output
Hello, Morrow!
```

`morrow run` compiles the file to a native executable in a private temporary
directory, runs it and forwards its exit code. `println` writes its argument and a
newline; `print` writes without the newline.

## Running, building and checking

### Running a program

`morrow run source.mr` compiles and runs in one step. Arguments after `--` go to the
program and are available through `System.args()`:

```sh
morrow run tool.mr -- input.txt --verbose
```

### Building an executable

`morrow build` keeps the executable. Use `-o` to name it:

```sh
morrow build hello.mr -o hello
./hello
```
```output
Created executable: hello
Hello, Morrow!
```

The result is a self-contained native binary that links the Morrow runtime
statically; it depends only on the platform's system libraries.

### Checking without running

`morrow check` parses and typechecks a file without linking or running it. It is
the fastest feedback loop while editing:

```sh
morrow check hello.mr
```
```output
No type errors
```

Diagnostics name the file, line and column, then the message and the offending
source line. This program binds a `String` where an `Int` is declared:

```morrow ignore
fn main():
    let count: Int = "three"
    println(count)
```

It does not compile:

```output
typeerror.mr:2:22: error: function return: let annotation: expression type: expected Int, found String
      let count: Int = "three"
```

The check exits with status 1. Fix the first reported error and check again; a
later error is often a consequence of an earlier one. `--color=never` produces
plain output for logs and editors.

### Formatting

`morrow fmt source.mr` rewrites a file into the canonical layout: four-space
indentation, one space around operators and after commas, and explicit
parentheses around every binary operation. `morrow fmt --check` reports whether a
file would change without writing it, which suits continuous integration. Both
accept a directory and process every `.mr` file below it.

> [!NOTE]
> The formatter parenthesizes nested operators in full: `1 + 2 * 3` becomes
> `(1 + (2 * 3))`. Examples in this guide are written the way the parser reads
> them, without those parentheses; both spellings mean the same thing.

### The REPL

`morrow repl` evaluates expressions interactively and prints each value with its
type. Bindings and function definitions persist for the session:

```text
> let greeting = "Hello"
> fn twice(n: Int) -> Int: n * 2
> twice(21)
42 : Int
> "{greeting}, {twice(1)}"
"Hello, 2" : String
> [1, 2, 3] |> List.map((n) -> n + 1)
[2, 3, 4] : List(Int)
```

A rejected entry has no effect: the previous bindings stay as they were. The REPL
supports Tab completion and keeps history in `~/.morrow_history`. See
[Tooling](../TOOLING_PARITY.md) for the remaining commands (`morrow test`, `morrow doc`,
`morrow lsp`).

## Comments

A `#` starts a comment that runs to the end of the line. `/* ... */` opens a
block comment that may span several lines or sit inside a line.

```morrow
# A line comment runs to the end of the line.
fn main():
    let answer = 42  # Trailing comments are fine.
    /*
    A block comment can span
    several lines.
    */
    let text = /* or sit inside a line */ "ok"
    println(answer)
    println(text)
```
```output
42
ok
```

Documentation is not a comment: `@doc """..."""` before a declaration and
`@moduledoc """..."""` at the top of a file attach Markdown documentation that
`morrow doc` renders and `morrow test --doc` executes. See
[Testing and documentation](../DOCUMENTATION.md).

## Program structure

A source file is a sequence of declarations: functions, types, traits,
implementations, constants and imports. Declarations can appear in any order;
a function may call another function declared later in the file.

### The main function

An executable needs a `main` function. `fn main():` without a return type returns
`Unit` and the process exits with status 0 after the body finishes.

```morrow
fn greet(name: String) -> String:
    "Hello, {name}"

fn main():
    println(greet("Morrow"))
```
```output
Hello, Morrow
```

### Exit codes

Declare `fn main() -> Int` to choose the exit status. The value of the last
expression becomes the process exit code:

```morrow
fn main() -> Int:
    println("failing on purpose")
    3
```
```output
failing on purpose
```

```sh
morrow run exit.mr; echo "exit=$?"
```
```output
failing on purpose
exit=3
```

`main` may also return `Result((), e)` with a concrete error type `e`. An `Ok(())` exit succeeds; an `Err` exit prints
`morrow: main returned Err` and exits with status 1. This is the natural shape
when `main` uses the `?` operator, covered in
[Error handling](error-handling.md).

```morrow
fn main() -> Result((), String):
    println("starting")
    Ok(())
```
```output
starting
```

A runtime fault such as division by zero or an out-of-range list index also
terminates the program with status 1 and a `morrow: runtime error: ...` message.

## Modules and imports

Every source file is a module. A single-file program does not need to say so;
larger programs split into files under one project directory and import each
other by module path.

### Files and module paths

A module path is a dotted name that mirrors the file path below the project
root: `geometry.shapes` is `geometry/shapes.mr`, or `geometry/shapes/mod.mr` for
a module that owns a directory. A file opens with a `module` declaration naming
its own path; the compiler rejects a declaration that does not match the file
location. A file without a `module` line takes its file stem as its name.

For an ordinary `main.mr` entry without a module declaration, the project root
is the directory containing that file. A declared module path determines how
far to walk upward: `module geometry.shapes` in `geometry/shapes.mr` resolves
imports from the directory containing `geometry`. Thus `morrow run main.mr`
finds `geometry/shapes.mr` next to `main.mr`.

Given `geometry/shapes.mr`:

```morrow
module geometry.shapes

pub type Shape:
    Circle(Int)
    Rectangle(Int, Int)

pub fn area(shape: Shape) -> Int:
    match shape:
        Circle(radius) -> 3 * radius * radius
        Rectangle(width, height) -> width * height

pub fn describe(shape: Shape) -> String:
    "{label(shape)} with area {area(shape)}"

fn label(shape: Shape) -> String:
    match shape:
        Circle(_) -> "circle"
        Rectangle(_, _) -> "rectangle"
```

`main.mr` in the project root imports the module and refers to its members by
their full path. The file does not compile on its own; the compiler needs
`geometry/shapes.mr` beside it:

```morrow ignore
import geometry.shapes

fn main():
    let box = geometry.shapes.Rectangle(3, 4)
    println(geometry.shapes.area(box))
    println(geometry.shapes.describe(box))
```
```output
12
rectangle with area 12
```

Constructors are reachable both directly (`geometry.shapes.Rectangle`) and
through their type (`geometry.shapes.Shape.Rectangle`).

### Visibility

Declarations are private to their module unless marked `pub`. `pub` applies to
functions, types, newtypes, traits, constants and imports. In the module above,
`area` and `describe` are public and `label` is private. Calling
`geometry.shapes.label(...)` from another file is an error:

```output
main.mr:4:13: error: geometry.shapes.label is private or not exported by its imported module
```

A `pub type` exports the type together with its constructors and fields. Public
functions must annotate every parameter and the return type, because their
signature is the module's interface; private functions may leave annotations out
and let the compiler infer them (see
[Type annotations and inference](syntax-and-values.md#type-annotations-and-inference)).

### Importing

`import a.b` brings the module in under its full path. Three variations shorten
the names at the use site. Each of the following files needs `geometry/shapes.mr`
from above and does not compile on its own.

A selective import lists the names to bring into scope unqualified:

```morrow ignore
import geometry.shapes.{Shape, area}

fn main():
    println(area(Shape.Circle(2)))
```
```output
12
```

Selecting a type also makes its constructors available as `Shape.Circle`;
selecting a constructor by name (`{Circle, area}`) makes `Circle` available
directly. An alias renames the module prefix:

```morrow ignore
import geometry.shapes as shapes

fn main():
    println(shapes.area(shapes.Circle(1)))
```
```output
3
```

A wildcard import brings every public name into scope unqualified. It is
convenient in small programs and hides where names come from in large ones:

```morrow ignore
import geometry.shapes.*

fn main():
    println(area(Circle(1)))
```
```output
3
```

Importing a name that the module does not export is an error, as is an import
cycle between two modules. Extract the shared declarations into a third module
that both import.

### Re-exports

`pub import` re-exports selected names from another module. A `mod.mr` file
often collects the public surface of a directory this way. `geometry/mod.mr`
imports `geometry/shapes.mr`, so it does not compile without that file:

```morrow ignore
module geometry

pub import geometry.shapes.{Shape, area}

pub fn unit_square() -> Shape:
    Shape.Rectangle(1, 1)
```

A user then imports `geometry` alone. This file needs both `geometry/mod.mr` and
`geometry/shapes.mr` and does not compile on its own:

```morrow ignore
import geometry

fn main():
    println(geometry.area(geometry.unit_square()))
    println(geometry.area(geometry.Shape.Circle(3)))
```
```output
1
27
```

Re-exported names keep their original interfaces, including any labeled
arguments.

### Built-in modules

`String`, `List`, `Map`, `Set`, `Option`, `Result`, `Int`, `System` and the
service modules `fs`, `json`, `http`, `sql` and `actors` are available in every
file without an import. The [standard library](../STDLIB_API_REFERENCE.md) chapter lists
them.

## Where to go next

- [Syntax and values](syntax-and-values.md): layout rules, literals, operators
  and type annotations.
- [Labeled calls](../LABELED_CALLS.md): function argument labels and positional calls.
- [Values and control flow](../LANGUAGE_GUIDE.md#choose-a-result-and-work-with-lists):
  matching results and working with lists.
- [Types](types.md) and [Traits](value-traits.md): records, tagged sums, newtypes,
  unions and static dispatch.
- [Error handling](error-handling.md): `Result`, `Option` and the obligation to
  handle every error.
- The [language tour](../../examples/language_tour.mr) combines traits,
  compile-time constants, sets, JSON and cleanup in one tested program.
