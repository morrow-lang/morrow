# Traits

A trait names a set of functions that several types can provide. This chapter covers
declaring and implementing traits, default methods, parent traits, bounds on generic
functions, the built-in value traits and how derivation and sorting use them.

Morrow traits are resolved statically. Each trait has exactly one type parameter, the
compiler selects the implementation while checking and specializing the program, and
the executable contains ordinary functions. There is no runtime method table.

## Defining a trait

`trait Name(a):` introduces a trait over the type parameter `a`. Its body lists method
signatures. Parameter and result types are written out; the parameter of type `a` is
where dispatch happens.

```morrow
trait Label(a):
    fn label(value: a) -> String
```

A trait method is called like a function: `label(task)`. Methods that take several
parameters follow the ordinary [labeled argument](../LABELED_CALLS.md) rules, and callers use
the labels written on the trait method.

## Implementing a trait

`impl Trait(Type):` provides the methods for one type. Every method of the trait without
a default body must appear, and each method's signature must match the trait with `a`
replaced by the type.

```morrow
trait Label(a):
    fn label(value: a) -> String

type Task:
    title: String

impl Label(Task):
    fn label(value: Task) -> String:
        value.title

impl Label(Int):
    fn label(value: Int) -> String:
        "number {value}"

fn main():
    println(label(Task("write docs")))
    println(label(42))
```
```output
write docs
number 42
```

A module may implement a trait when it owns the trait or the target type. Owning the
trait is enough to implement it for built-in types such as `Int`, as above. Implementing
a built-in trait for a built-in type in your own module is rejected:

```text
foreign.mr:1:1: error: an implementation must belong to the module defining its trait or type
  impl Show(Int):
```

Two implementations for the same type are rejected with `overlapping implementation`.

Implementation methods may use several pattern clauses, and every clause must satisfy
the trait signature and the usual coverage rules:

```morrow
trait Describe(a):
    fn describe(value: a) -> String

type Status:
    Active
    Suspended(reason: String)

impl Describe(Status):
    fn describe(Active: Status) -> String: "active"
    fn describe(Suspended(reason): Status) -> String: "suspended: {reason}"

fn main():
    println(describe(Active))
    println(describe(Suspended("billing")))
```
```output
active
suspended: billing
```

## Default methods

A method with a body in the trait declaration is a default. Implementations inherit it
and may override it. Defaults can call the trait's other methods.

```morrow
trait Greet(a):
    fn name(value: a) -> String
    fn greet(value: a) -> String:
        "Hello, {name(value)}"

type Person:
    first: String

type Robot:
    serial: Int

impl Greet(Person):
    fn name(value: Person) -> String: value.first

impl Greet(Robot):
    fn name(value: Robot) -> String: "unit {value.serial}"
    fn greet(value: Robot) -> String: "BEEP {name(value)}"

fn main():
    println(greet(Person("Ada")))
    println(greet(Robot(7)))
```
```output
Hello, Ada
BEEP unit 7
```

## Parent traits

`trait Child(a) with Parent(a):` requires every type implementing `Child` to implement
`Parent` too. Methods of the child, including defaults, may call parent methods. The
built-in `Ord` is declared this way over `Eq`.

```morrow
trait Shape(a):
    fn area(value: a) -> Float

trait Solid(a) with Shape(a):
    fn height(value: a) -> Float
    fn volume(value: a) -> Float:
        area(value) * height(value)

type Cylinder:
    radius: Float
    length: Float

impl Shape(Cylinder):
    fn area(value: Cylinder) -> Float: 3.0 * value.radius * value.radius

impl Solid(Cylinder):
    fn height(value: Cylinder) -> Float: value.length

fn main():
    println(volume(Cylinder(1.0, 2.0)))
```
```output
6
```

Implementing `Solid(Cylinder)` without `Shape(Cylinder)` is an error, and a cycle of
parent declarations is rejected.

## Bounds on generic functions

A generic function that calls a trait method states the requirement with a `where`
clause after the return type. Several bounds are separated by commas.

```morrow
fn largest(items: List(a), fallback: a) -> a where Ord(a):
    List.fold(items, fallback, (best, item) ->
        match compare(left: item, right: best):
            Greater -> item
            _ -> best
    )

fn show_all(items: List(a)) -> String where Show(a):
    String.join(List.map(items, (item) -> show(item)), ", ")

fn main():
    println(largest([3, 9, 4], 0))
    println(largest(["pear", "apple"], ""))
    println(show_all([1, 2, 3]))
    println(show_all(["a", "b"]))
```
```output
9
pear
1, 2, 3
"a", "b"
```

Calling a bounded function with a type that lacks the implementation fails at the call
site. For `describe(1.5)` where `describe` requires `Label(a)` in `labels.mr`:

```text
labels.mr:8:13: error: type Float has no implementation of labels.Label
      println(describe(1.5))
```

A bound is enforced even when the body never calls the method, so a `where Label(a)`
on a function that returns its argument unchanged still rejects `Int` arguments. When
a generic function calls a trait method without writing a bound, the compiler infers
the requirement from the body and reports a missing implementation at each concrete
call instead of in the signature. Writing the bound keeps the contract in the
signature, where readers and editor hover see it.

## Generic implementations

An `impl` may target a generic type and state bounds on the type's parameters. The
implementation applies to every instantiation whose arguments satisfy the bounds.

```morrow
trait Label(a):
    fn label(value: a) -> String

type Box(a):
    value: a

impl Label(Int):
    fn label(value: Int) -> String: "{value}"

impl Label(Box(a)) where Label(a):
    fn label(value: Box(a)) -> String:
        "Box({label(value.value)})"

fn main():
    println(label(Box(Box(3))))
```
```output
Box(Box(3))
```

## Static dispatch

Trait methods are resolved at check time and compiled to ordinary functions. A method
name can therefore be used as a function value wherever the element type is concrete:

```morrow
trait Label(a):
    fn label(value: a) -> String

type Task:
    title: String

impl Label(Task):
    fn label(value: Task) -> String: value.title

fn main():
    println(List.map([Task("a"), Task("b")], label))
    let describe = label
    println(describe(Task("c")))
```
```output
["a", "b"]
c
```

There are no trait objects: a value's type is always one concrete type, and a
`List(a) where Label(a)` holds elements of one type per instantiation. To hold values of
several types in one collection, use a [tagged sum](types.md#tagged-sums) or a
[union](types.md#unions) and match on it.

## Built-in traits

The prelude declares these traits and the `Ordering` sum. They become available
automatically when a program uses `derive`, an `impl`, a `where` bound or prints a
structured value.

| Trait | Method | Meaning |
| --- | --- | --- |
| `Show(a)` | `show(value: a) -> String` | structural text used by `print` and `println` |
| `Eq(a)` | `eq(left: a, right: a) -> Bool`, `neq` (default) | structural equality |
| `Ord(a) with Eq(a)` | `compare(left: a, right: a) -> Ordering` | ordering: `Less`, `Equal`, `Greater`; see the Float caveat below |
| `Clone(a)` | `clone(value: a) -> a` | reconstruct a structural value |
| `Json(a)` | `to_json`, `from_json` | JSON codec; see [JSON](../JSON_RUST_API.md) |

`eq`, `neq` and `compare` take labeled arguments `left:` and `right:`. The built-in
implementations cover:

- `Int`, `Float`, `Bool`, `String` and `()`: all four value traits. `Ord(Float)`
  follows `<` and `>`, so NaN compares `Equal` to everything; `Ord(Bool)` places
  `false` before `true`; `Ord(String)` orders by UTF-8 bytes like `String.compare`.
- `List(a)`, `Option(a)`, `Result(a, e)` and tuples: each trait when the element types
  have it. Lists compare lexicographically; `Some` orders before `None` and `Ok` before
  `Err`, following declaration order.
- `Map(k, v)`: `Show`, `Eq` and `Clone`. Equality ignores insertion order. Maps have no
  `Ord`.
- `Set(a)`: none. Convert with `Set.to_list` or use `Set.equal`.

```morrow
fn name(order: Ordering) -> String:
    match order:
        Less -> "Less"
        Equal -> "Equal"
        Greater -> "Greater"

fn main():
    println(name(compare(left: 0.0 / 0.0, right: 1.0)))
    println(name(compare(left: [1, 2], right: [1, 3])))
    println(name(compare(left: "b", right: "a")))
    println(name(compare(left: Ok(1), right: Err("x"))))
    println(eq(left: %{"a": 1}, right: %{"a": 1}))
    println(neq(left: (1, "a"), right: (1, "a")))
```
```output
Equal
Less
Greater
Less
true
false
```

`Ordering` itself has no `Show`, which is why the example converts it to text. The
operators `==`, `<` and friends keep their intrinsic scalar semantics and do not
dispatch through `Eq` or `Ord`; on records and sums they are rejected, so use `eq` and
`compare` there.

## Deriving

`derive(Show, Eq, Ord, Clone)` on a record, sum or newtype generates the
implementations structurally. The generated code is ordinary checked Morrow: fields print
in declaration order as `Name(field: value, ...)`, records and tuples compare field by
field, and sum variants compare by declaration position before their payloads.

```morrow
type Point derive(Show, Eq, Ord, Clone):
    x: Int
    y: Int

fn name(order: Ordering) -> String:
    match order:
        Less -> "Less"
        Equal -> "Equal"
        Greater -> "Greater"

fn main():
    println(show(Point(1, 2)))
    println(eq(left: Point(1, 2), right: clone(Point(1, 2))))
    println(name(compare(left: Point(1, 2), right: Point(1, 3))))
```
```output
Point(x: 1, y: 2)
true
Less
```

`derive(Ord)` requires `Eq` on the same declaration; `type Point derive(Ord)` alone
fails with `type ordeq.Point has no implementation of Eq; add derive(Eq) to its
declaration or write an impl`, where `ordeq` is the module name. Derivation works for generic and
recursive types when every field type supports the trait, and phantom parameters
acquire no bounds.

You can mix derived and hand-written implementations. Here `Show` and `Eq` are derived
while `Ord` compares only one field:

```morrow
type Celsius derive(Show, Eq):
    degrees: Float

impl Ord(Celsius):
    fn compare(left: Celsius, right: Celsius) -> Ordering:
        compare(left: left.degrees, right: right.degrees)

fn main():
    println(List.sort([Celsius(21.5), Celsius(-3.0)]))
```
```output
[Celsius(degrees: -3), Celsius(degrees: 21.5)]
```

A hand-written `Show` replaces the structural text everywhere the value prints,
including inside lists:

```morrow
type Point:
    x: Int
    y: Int

impl Show(Point):
    fn show(value: Point) -> String:
        "({value.x}, {value.y})"

fn main():
    println([Point(1, 2), Point(3, 4)])
```
```output
[(1, 2), (3, 4)]
```

## Sorting with Ord

`List.sort` returns a new ascending list. For `Int`, `Float`, `Bool`, `String` and
newtypes over them it uses the runtime scalar orders. For every other element type,
including generic parameters, it calls the `Ord` method `compare`, so records need
`derive(Ord)` or an `impl`. `List.sort_by` takes any comparator returning `Ordering`.
Both sorts are stable: elements that compare `Equal` keep their input order.

```morrow
type Task derive(Show):
    priority: Int
    title: String

fn by_priority(left: Task, right: Task) -> Ordering:
    compare(left: left.priority, right: right.priority)

fn main():
    let tasks = [Task(2, "write"), Task(1, "read"), Task(2, "review")]
    println(List.sort_by(tasks, by_priority))
    println(List.sort_by([3, 1, 2], (a, b) -> compare(left: b, right: a)))
    println(List.sort([(2, "b"), (1, "z"), (2, "a")]))
```
```output
[Task(priority: 1, title: "read"), Task(priority: 2, title: "write"), Task(priority: 2, title: "review")]
[3, 2, 1]
[(1, "z"), (2, "a"), (2, "b")]
```

Sorting a list of records without `Ord` is rejected at check time:

```text
sorting.mr:6:13: error: type sorting.Point has no implementation of Ord; add derive(Ord) to its declaration or write an impl
      println(List.sort([Point(1.0, 2.0)]))
```

Comparators run as ordinary Morrow calls, so a comparator may allocate or handle
`Result` values like any function. In the browser target `List.sort_by`, and therefore
`List.sort` on structured elements, is reported as an unavailable host capability.

## Limits

Trait resolution shares bounded work and depth budgets with the rest of the checker;
exhausting them is a diagnostic, not silent acceptance. The following are not part of
the language:

- Dynamic dispatch, trait objects or values typed by a trait.
- Multi-parameter traits and associated types.
- Operator overloading through `Eq` or `Ord`.
- Implementing a trait for a type when your module owns neither.

See [Traits and explicit structural derivation](../TRAITS.md) for the verification
evidence behind this chapter.
