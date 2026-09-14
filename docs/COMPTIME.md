# Compile-time constants

Fern evaluates `const` initializers while checking a program, using the same
integer, string, collection and function semantics as ordinary Fern code.

```fern
fn factorial(n: Int) -> Int:
    if n <= 1: 1
    else: n * factorial(n - 1)

const capacity: Int = comptime: factorial(6)

pub const greeting: String = comptime:
    "Hello, 🌿 Fern"

fn main():
    println(capacity)
    println(greeting)
```

Constants are values: write `capacity`, without parentheses. A constant can also
hold a function, which you call normally. Public constants require a type
annotation, participate in module exports, and appear as constants in LSP hover
and completion. Private constants can infer their types. Forward references
follow the same dependency analysis as functions. An otherwise unconstrained
empty collection or `None` requires an annotation, such as `const empty:
List(Int) = comptime: []`. Constants always have concrete types and are evaluated
even when the application never references them.

An initializer can call ordinary Fern functions, recurse, use local bindings,
match values, and construct lists, maps, sets, tuples, records, sums, newtypes,
Options, Results and captured functions. Result-handling rules still apply.
The compiler embeds the resulting typed data; it does not rerun the initializer
in the shipped application. Referencing aggregate data can allocate its runtime
representation.

Evaluation is deterministic and cannot print, access the filesystem, start
actors, or execute foreign calls. The compiler checks capabilities before an
effect occurs. Failed initialization produces a source diagnostic and leaves a
REPL session's previously accepted declarations intact.

One check shares a budget of 100,000 evaluator steps across initializers. Embedded
data shares a 100,000-node and 1 MiB string budget with a nesting limit of 128.
These limits turn unbounded recursion and excessive generated data into
diagnostics. The compiler runs no generated native executable or external
interpreter during evaluation.

The implemented syntax is top-level `const ... = comptime:`. General inline
compile-time expressions, AST reflection and macro generation are separate
language features. Opaque host resources cannot be embedded as constant data.
