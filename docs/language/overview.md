# Morrow language guide

Morrow is a statically typed, expression-oriented language with immutable values,
explicit errors and supervised actors. It compiles to native executables, runs
interactively in a REPL and targets WebAssembly for browser clients.

```morrow
fn main():
    println("Hello, Morrow!")
```

The chapters in this guide describe the implemented language. Each example is
typechecked by the repository test suite; features that are still proposals are
marked as such and live in the [design document](../../DESIGN.md).
