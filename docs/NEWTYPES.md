# Distinct newtypes in the Rust frontend

Newtypes give values a separate identity without allocating a wrapper:

```fern
newtype UserId = UserId(Int)
newtype ProductId = ProductId(Int)

fn raw(id: UserId) -> Int: id.0
fn main(): println(raw(UserId(42)))
```

A ProductId cannot be passed where UserId is required. Construct explicitly and
unwrap with `.0` or a constructor pattern. Other fields and implicit conversion
are rejected. Generic payloads may use a different constructor name:

```fern
newtype Wrapper(a) = Packed(a)
fn unwrap(Packed(value): Wrapper(a)) -> a: value
```

Construction and projection emit the same native operand. Int and Float retain
all 64 bits; Bool, Unit, containers, functions and native objects retain their
existing representation. A wrapper adds no allocation or tag/payload load to
these values. Collections and closures preserve the payload ABI, including Float
bit conversion at storage boundaries. The REPL also reuses underlying values and
uses checked type metadata to display their constructor identity.

Equality and inequality require the same exact nominal identity. Wrappers over
Int, Float, Bool or String inherit the current scalar comparison semantics,
including IEEE NaN/signed-zero rules and String content equality. List.contains
uses those same rules. Map keys support wrappers over Int, Bool and String;
wrapped Float remains an invalid key. Nested wrappers follow the same policy.
Arithmetic, ordering, printing and interpolation require explicit unwrapping;
general traits and derivation remain separate work.

Wrapping Result does not hide its handling obligations. Wildcard payload discards
and delayed captures retain the ordinary checker restrictions. A public newtype
exports its intended constructor; a public transparent alias does not export a
private target constructor. Source formatting, docs, hover, definitions and
incomplete `.0` completion retain the original owner and constructor locations.

An unboxed cycle has no representation and is rejected, including cycles whose
generic arguments grow on each expansion. Recursion behind an existing allocated
container is supported, for example `newtype Chain = Chain(List(Chain))`. Concrete
type keys permit finite nesting such as Wrapper(Wrapper(Int)). Representation and
capability expansion use depth 128, 4096-node type limits and an aggregate 400,000-node
work limit. The backend validates concrete layout shape and references before
selecting widths or emitting code, including caller-created IR.

This feature is available in the default Rust compiler, `fern`. The explicit
C reference compiler was removed during the Rust-only migration.
