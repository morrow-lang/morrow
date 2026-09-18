# Types

This chapter describes the types a Morrow program is built from: the scalar and
collection types the compiler knows about, the record, sum, alias, newtype and union
declarations you write yourself, generic type parameters, and structural derivation.

Morrow is statically typed. Every expression has one type that the compiler knows before
the program runs; local `let` bindings are inferred and function signatures state their
parameter and result types. Type names start with an uppercase letter (`Int`, `User`).
Lowercase names in type positions (`a`, `key`) are type parameters. Generic arguments
are written in parentheses: `List(Int)`, `Map(String, Int)`, `Result(Int, String)`.

## Scalar types

| Type | Values | Notes |
| --- | --- | --- |
| `Int` | signed 64-bit integers | `+`, `-` and `*` wrap; `/` and `%` fault on a zero divisor |
| `Float` | IEEE 754 doubles | no total order; see [Traits](value-traits.md#built-in-traits) |
| `Bool` | `true`, `false` | `and`, `or`, `not`, `==`, `!=` |
| `String` | immutable UTF-8 text | `String.len` counts bytes |

```morrow
fn main():
    let count: Int = 9007199254740993
    let ratio: Float = 2.5
    let ready: Bool = true
    let name: String = "Morrow 🌿"
    println(count)
    println(ratio)
    println(ready)
    println(name)
    println(7 / 2)
    println(-7 % 3)
    println(2 ** 10)
    println(String.len(name))
```
```output
9007199254740993
2.5
true
Morrow 🌿
3
-1
1024
11
```

Integer division truncates toward zero and the remainder takes the sign of the
dividend. Overflow wraps rather than faulting; `Int.checked_add` and the other checked
operations return `Option(Int)` when you need to detect it. Dividing by zero is a
runtime fault that runs deferred cleanup and then stops the program with a diagnostic;
see [Error handling](error-handling.md#no-exceptions).

```morrow
fn main():
    println(9223372036854775807 + 1)
    println(Int.checked_add(9223372036854775807, 1))
    println(Int.parse("42"))
    println(Int.parse("4x"))
```
```output
-9223372036854775808
None
Some(42)
None
```

Float text uses up to 17 significant digits to preserve the stored double when
read back, and whole floats print without a fraction.

```morrow
fn main():
    println(1.0)
    println(0.1 + 0.2)
    println(2.5 * 2.0)
```
```output
1
0.30000000000000004
5
```

Strings, interpolation with `{...}` and the `String` module are covered in
[Strings](syntax-and-values.md#string).

## Unit

`()` is the type and the value of "nothing". It is spelled `()` in both positions;
`Unit` is an accepted alias in type positions. A function whose body ends in a
Unit expression returns `()`, and a `let` may bind that value. Private functions
infer a missing return annotation; `main` without an annotation returns `()`.

```morrow
fn log(message: String) -> ():
    println(message)

fn nothing() -> Unit: ()

fn main():
    let done: () = log("side effect")
    let also: () = nothing()
    println("both bound")
```
```output
side effect
both bound
```

`()` implements `Show`, but direct `print` and `println` calls reject Unit arguments.
Use `println(show(done))` to print its `()` representation.

## Tuples

A tuple groups a fixed number of values of possibly different types. The type of
`(1, "one")` is `(Int, String)`. Read positions with `.0`, `.1` and so on, or
destructure with a tuple pattern. A one-element tuple needs a trailing comma: `(7,)`
has type `(Int,)`.

```morrow
fn swap(pair: (a, b)) -> (b, a):
    (pair.1, pair.0)

fn main():
    let pair: (Int, String) = (1, "one")
    let (number, text) = pair
    println(number)
    println(text)
    println(pair.0)
    println(swap(pair))
    println((1, 2.5, false))
```
```output
1
one
1
("one", 1)
(1, 2.5, false)
```

Tuples are positional. When the fields need names, declare a [record](#records)
instead.

## Collections

`List(a)`, `Map(k, v)` and `Set(a)` are immutable. Operations that update a collection
return a new collection and leave the original unchanged. List literals use brackets, map literals
use `%{key: value}`, and sets are built through `Set.from_list` or `Set.insert`.

```morrow
fn main():
    let numbers: List(Int) = [1, 2, 3]
    let ages: Map(String, Int) = %{"ada": 36, "alan": 41}
    let tags: Set(String) = Set.from_list(["a", "b", "a"])
    println(List.push(numbers, 4))
    println(numbers)
    println(Map.get(ages, "ada"))
    println(Map.get(ages, "grace"))
    println(Set.len(tags))
```
```output
[1, 2, 3, 4]
[1, 2, 3]
Some(36)
None
2
```

A list has one element type. Map keys may be `Int`, `Bool`, `String` or newtypes over
them; `Float` is not a valid key. Lookups return `Option(v)`. See the
[list examples](../../examples/list_advanced.mr) and [Set API](../SETS.md) for more operations.

## Option and Result

`Option(a)` is `Some(value)` or `None`. `Result(a, e)` is `Ok(value)` or `Err(error)`.
Morrow has no null value and no exceptions; these two types carry absence and failure,
and the compiler requires every `Result` to be handled. Both are ordinary tagged sums
that you match on.

```morrow
fn main():
    let maybe: Option(Int) = Some(3)
    let absent: Option(Int) = None
    println(maybe)
    println(absent)
    println(Option.unwrap_or(absent, 0))
    let outcome: Result(Int, String) = Err("boom")
    match outcome:
        Ok(value) -> println(value)
        Err(message) -> println(message)
```
```output
Some(3)
None
0
boom
```

[Error handling](error-handling.md) covers `?`, `with`, `defer` and the handling
obligation in depth.

## Records

A `type` declaration with named fields defines a record. Construct it with the type
name and positional arguments in field order, read fields with `.name`, and build a
changed copy with the update syntax `%{ value | field: new }`. The original is
unchanged.

```morrow
type User derive(Show):
    name: String
    email: String
    age: Int

fn main():
    let user = User("Ada", "ada@example.com", 36)
    println(user.name)
    let older = %{ user | age: 37 }
    println(older)
    println(user)
```
```output
Ada
User(name: "Ada", email: "ada@example.com", age: 37)
User(name: "Ada", email: "ada@example.com", age: 36)
```

Constructors take positional arguments; `User(name: "Ada", ...)` is rejected with
`this callable uses positional arguments`. Labeled arguments apply to functions, as
described in [Labeled calls](../LABELED_CALLS.md).

Records can refer to themselves through `Option`, `List` or another allocated container:

```morrow
type Node derive(Show):
    value: Int
    next: Option(Node)

fn main():
    println(Node(1, Some(Node(2, None))))
```
```output
Node(value: 1, next: Some(Node(value: 2, next: None)))
```

## Tagged sums

A `type` whose body lists variants defines a tagged sum. Each variant is a constructor
with zero or more payload fields. Payload fields may be named in the declaration for
documentation; construction is positional. `match` selects a variant and binds its
payload, and the compiler checks that every variant is covered.

```morrow
type Shape derive(Show):
    Circle(radius: Float)
    Rectangle(width: Float, height: Float)
    Point

fn area(shape: Shape) -> Float:
    match shape:
        Circle(radius) -> 3.0 * radius * radius
        Rectangle(width, height) -> width * height
        Point -> 0.0

fn main():
    println(area(Circle(1.0)))
    println(area(Rectangle(2.0, 3.5)))
    println([Circle(1.0), Rectangle(2.0, 3.5), Point])
```
```output
3
7
[Circle(1), Rectangle(2, 3.5), Point]
```

Sums may be recursive, and they are the usual way to model trees and custom error
types:

```morrow
type Tree(a):
    Leaf(value: a)
    Branch(left: Tree(a), right: Tree(a))

fn sum(tree: Tree(Int)) -> Int:
    match tree:
        Leaf(value) -> value
        Branch(left, right) -> sum(left) + sum(right)

fn main():
    println(sum(Branch(Leaf(4294967296), Branch(Leaf(20), Leaf(22)))))
```
```output
4294967338
```

Patterns, guards and exhaustiveness are described in
[Control flow](../LANGUAGE_GUIDE.md#choose-a-result-and-work-with-lists).

## Type aliases

`type Name = Type` introduces a transparent alias. The alias and its target are the
same type: a `Score` is an `Int` and can be used wherever an `Int` is expected. Aliases
may take parameters.

```morrow
type Score = Int
type Headers = Map(String, String)
type Pairs(a) = List((a, a))

fn total(scores: List(Score)) -> Score: List.sum(scores)

fn main():
    let headers: Headers = %{"content-type": "text/plain"}
    println(Map.get(headers, "content-type"))
    let plain: Int = total([1, 2, 3])
    println(plain)
    let pairs: Pairs(Int) = [(1, 2), (3, 4)]
    println(pairs)
```
```output
Some("text/plain")
6
[(1, 2), (3, 4)]
```

Function types are written `(Int) -> String`; the `fn(Int) -> String` spelling is also
accepted. Use an alias when a function type appears in several signatures.

## Newtypes

A newtype wraps one payload in a distinct nominal type without allocating. Two newtypes
over the same payload are different types, so the compiler rejects passing a
`ProductId` where a `UserId` is required. Unwrap with `.0` or a constructor pattern.

```morrow
newtype UserId = UserId(Int)
newtype ProductId = ProductId(Int)

fn describe(id: UserId) -> String:
    "user #{id.0}"

fn raw(UserId(value): UserId) -> Int: value

fn main():
    let id = UserId(42)
    println(describe(id))
    println(raw(id))
    println(id == UserId(42))
```
```output
user #42
42
true
```

Passing `ProductId(42)` to `describe` does not compile. `bin/morrow check ids.mr` reports:

```text
ids.mr:8:22: error: function return: call argument: call argument: call result: expected ids.UserId, found ids.ProductId
      println(describe(ProductId(42)))
```

Arithmetic and interpolation need the payload, not the wrapper; printing needs
`Show`, and trait-based comparison needs `Ord`:
`Meters(1) + Meters(2)` is rejected with `addition operator requires Int, Float, or
String operands`. `==` compares wrappers of the same newtype by payload. A generic
newtype may use a constructor name that differs from the type name, and newtypes accept
`derive`:

```morrow
newtype Wrapper(a) = Packed(a)
newtype UserId derive(Show, Eq, Ord, Clone) = UserId(Int)

fn unwrap(Packed(value): Wrapper(a)) -> a: value

fn main():
    println(unwrap(Packed("text")))
    println(List.sort([UserId(3), UserId(1), UserId(2)]))
```
```output
text
[UserId(1), UserId(2), UserId(3)]
```

Construction and `.0` compile to the same machine operand as the payload; wrapping an
`Int` costs nothing at runtime. A newtype cannot contain itself directly, but recursion
through a `List` or another allocated container is allowed. Details and limits are in
[Newtypes](../NEWTYPES.md).

## Unions

A union type accepts any of an explicit, finite set of member types. Write the members
separated by `|`, usually behind an alias. A value of union type cannot be used with a
member-specific operation until a typed pattern narrows it: `name: Type` in a `match`
arm binds `name` at that member's type.

```morrow
type Value = Int | String

fn describe(value: Value) -> String:
    match value:
        n: Int -> "number: {n}"
        s: String -> "text: {s}"

fn main():
    println(describe(4294967296))
    println(describe("morrow"))
    let items: List(Value) = [1, "two", 3]
    for item in items:
        println(describe(item))
```
```output
number: 4294967296
text: morrow
number: 1
text: two
number: 3
```

An arm may select a subset of members with `name: A | B`, and the match must cover
every member; a guard does not count as coverage. Passing an `Int` where `Int | String`
is declared is allowed, as is widening a smaller union into a larger one. The compiler
does not invent a union from unrelated branch types: both branches of an `if` still need
the same type unless an annotation supplies the union.

```morrow
type Value = Int | String | Bool

fn classify(value: Value) -> String:
    match value:
        n: Int -> "number {n}"
        other: String | Bool -> "not a number"

fn widen(n: Int) -> Value: n

fn main():
    println(classify(true))
    println(classify(widen(1)))
```
```output
not a number
number 1
```

Operations that need one concrete type reject an unnarrowed union. `value + 1` on an
`Int | String` fails with `expected Int, found Int | String`, and a union has no `Show`,
so `println(value)` is rejected until you match. Members flatten and deduplicate:
`Int | Int` is `Int`, and `(Int) -> String | Bool` is a function returning a union;
write `((Int) -> String) | Bool` for a union containing a function. Containers are
invariant: a `List(Int)` is not a `List(Int | String)`. The complete contract is in
[Unions](../UNIONS.md).

> [!NOTE]
> Constructor refinements such as `Ok(data) | Err(msg)` as a type, implicit union joins
> across branches and lifted capabilities are proposals in [DESIGN.md](../../DESIGN.md);
> they are not implemented. See [Language status](../LANGUAGE_STATUS.md).

## Generic types

Records and sums take type parameters in parentheses after the name. Parameters are
lowercase identifiers and are substituted at each use; `Box(Int)` and `Box(String)` are
distinct concrete types produced from one declaration.

```morrow
type Box(a) derive(Show):
    value: a

type Pair(a, b) derive(Show):
    left: a
    right: b

fn swap(pair: Pair(a, b)) -> Pair(b, a):
    Pair(pair.right, pair.left)

fn main():
    println(Box(1))
    println(Box("text"))
    println(swap(Pair(1, "one")))
```
```output
Box(value: 1)
Box(value: "text")
Pair(left: "one", right: 1)
```

A parameter that appears in no field is a phantom parameter. It still distinguishes
types, which is useful for tagging quantities with a unit:

```morrow
type Quantity(unit):
    amount: Int

type Meters:
    Meters

fn main():
    let distance: Quantity(Meters) = Quantity(5)
    println(distance.amount)
```
```output
5
```

Function signatures and their inferred types are covered in
[Type annotations and inference](syntax-and-values.md#type-annotations-and-inference);
`where` bounds are covered in [Traits](value-traits.md#bounds-on-generic-functions).

## Deriving traits

`derive(...)` after a record, sum or newtype name asks the compiler to generate
structural implementations of the built-in traits `Show`, `Eq`, `Ord` and `Clone`.
`Ord` requires `Eq` on the same type. Fields are compared and printed in declaration
order; variants of a sum order by declaration order before their payloads.

```morrow
type Point derive(Show, Eq, Ord, Clone):
    x: Int
    y: Int

type Priority derive(Show, Eq, Ord):
    Low
    Medium
    High

fn main():
    println(List.sort([Point(2, 1), Point(1, 9), Point(1, 2)]))
    println(List.sort([High, Low, Medium]))
    println(eq(left: Point(1, 2), right: Point(1, 2)))
    println(clone(Point(4, 5)))
```
```output
[Point(x: 1, y: 2), Point(x: 1, y: 9), Point(x: 2, y: 1)]
[Low, Medium, High]
true
Point(x: 4, y: 5)
```

Derivation works for generic and recursive declarations when the field types support
the trait. `derive(Json)` generates a JSON codec; see [JSON](../JSON_RUST_API.md). Writing your own
implementation instead of deriving, and the exact contracts of each trait, are the
subject of [Traits](value-traits.md).

## How values print

`print` and `println` write `Int`, `Float`, `Bool` and `String` values directly. Every
other argument goes through the `Show` trait, so lists, maps, tuples, options, results
and derived types print without calling `show` yourself. Inside a structured value,
strings render as Morrow literals with `"` and escapes, which keeps empty and spaced
strings visible. `show(text)` returns that literal form; `println(text)` prints raw
text.

```morrow
type Point derive(Show):
    x: Int
    y: Int

fn main():
    println("plain")
    println(show("plain"))
    println(["", "say \"hi\""])
    println(Some(Point(1, 2)))
    println(%{"xs": [1, 2]})
    println(List.zip([1, 2], ["x", "y"]))
```
```output
plain
"plain"
["", "say \"hi\""]
Some(Point(x: 1, y: 2))
%{"xs": [1, 2]}
[(1, "x"), (2, "y")]
```

A declared type without `Show` is rejected at check time. For a `Point` record declared
without `derive` in `shapes.mr`:

```text
shapes.mr:6:13: error: type shapes.Point has no implementation of Show; add derive(Show) to its declaration or write an impl
      println(Point(1, 2))
```

Type names in diagnostics carry their module prefix, here the file name `shapes`.

Functions, `Set(a)`, `Ordering` and unnarrowed unions have no `Show` implementation.
Convert them first, for example with `Set.to_list` or a `match` on the `Ordering`.
Unit implements `Show`, but direct printing still requires `show(())` first.
