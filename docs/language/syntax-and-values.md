# Syntax and values

This chapter covers the shape of Morrow source: layout and indentation, `let`
bindings, the literal forms of each built-in value, operators and their
precedence, and how types are written and inferred.

## Layout

Morrow uses indentation instead of braces. A line that ends in `:` introduces a
*suite*: an indented block of statements that belongs to the construct on that
line. The block ends when the indentation returns to the enclosing level.

```morrow
fn main():
    let limit = 3
    for n in 1..=limit:
        if n % 2 == 1:
            println("{n} is odd")
        else:
            println("{n} is even")
    println("done")
```
```output
1 is odd
2 is even
3 is odd
done
```

The formatter writes four spaces per level, and this guide does the same. The
parser accepts any consistent width, and tabs, but a file may not mix tabs and
spaces, and every statement in a block must start at the same column:

```output
bad.mr:3:1: error: inconsistent indentation
     println(2)
```

### One-line bodies

When a body is a single expression, it may follow the `:` on the same line. This
applies to functions and `if`/`else` bodies. Match arms and lambdas instead
introduce their expressions with `->`:

```morrow
fn square(n: Int) -> Int: n * n

fn main():
    let label = if square(3) > 5: "big" else: "small"
    println(label)
```
```output
big
```

### Continuation lines

An expression continues across lines inside parentheses, brackets or braces.
A pipeline may also continue on a new line that begins with `|>`:

```morrow
fn main():
    let total = List.fold(
        [1, 2, 3],
        0,
        (acc, n) -> acc + n,
    )
    let doubled = [1, 2, 3]
        |> List.map((n) -> n * 2)
        |> List.sum()
    println(total)
    println(doubled)
```
```output
6
12
```

A binary operator at the end of a line does not continue the expression; wrap
the expression in parentheses instead.

## Bindings

### let

`let` binds a name to a value. The type is inferred from the value; an annotation
after the name states it explicitly.

```morrow
let count = 42
let ratio: Float = 0.5
let names = ["ada", "grace"]
println("{count} {ratio} {List.len(names)}")
```

Bindings are immutable. There is no assignment operator and no `mut`; a value,
once bound, does not change. Collections follow the same rule: `List.push`
returns a new list and leaves its input intact.

```morrow
fn main():
    let scores = [10, 20]
    let extended = List.push(scores, 30)
    println(scores)
    println(extended)
```
```output
[10, 20]
[10, 20, 30]
```

### Rebinding

A new `let` may reuse a name. It introduces a fresh binding that shadows the old
one from that point on; the earlier value is not modified. The new binding may
even have a different type.

```morrow
fn main():
    let x = 1
    let x = x + 1
    let x = "now a string: {x}"
    println(x)
```
```output
now a string: 2
```

### Destructuring

A `let` pattern takes a tuple, constructor or list apart. The pattern must be
irrefutable; a pattern that can fail needs `let ... else` or `match`, see
[Control flow](../LANGUAGE_GUIDE.md#choose-a-result-and-work-with-lists).

```morrow
fn main():
    let (name, year) = ("Morrow", 2026)
    println("{name} {year}")
```
```output
Morrow 2026
```

## Literals

### Int

`Int` is a signed 64-bit integer, and every value uses the full width: the range
is `-9223372036854775808` to `9223372036854775807`. Underscores group digits,
and `0x`, `0o` and `0b` prefixes write hexadecimal, octal and binary literals.

```morrow
fn main():
    println(9223372036854775807)
    println(-9223372036854775808)
    println(1_000_000)
    println(0xFF)
    println(0o17)
    println(0b1010)
```
```output
9223372036854775807
-9223372036854775808
1000000
255
15
10
```

A literal outside the 64-bit range is a compile-time error. The arithmetic
operators wrap on overflow; see [Checked integer arithmetic](#checked-integer-arithmetic)
below for the alternatives.

### Float

`Float` is an IEEE 754 double. A literal has a decimal point with digits on both
sides, or an exponent: `2.5`, `0.5`, `1.5e3`. `Int` and `Float` never mix
implicitly; `1.5 + 2` is a type error.

```morrow
fn main():
    println(1.5 + 2.25)
    println(7.0 / 2.0)
    println(1.5e3)
    println(0.1 + 0.2)
```
```output
3.75
3.5
1500
0.30000000000000004
```

Printing a `Float` uses 17 significant digits and drops trailing zeros, so `2.5`
prints as `2.5`, `2.0` prints as `2`, and `3.14` prints as
`3.1400000000000001`, enough digits to preserve the stored double when read back. `Float` supports
`+ - * / **` and the comparison operators; `%` is defined for `Int` only.

### Bool

`true` and `false`. The boolean operators are the words `and`, `or` and `not`;
`and` and `or` short-circuit.

```morrow
fn main():
    let ready = true
    println(ready and not false)
    println(false or 1 < 2)
```
```output
true
true
```

### String

A `String` is UTF-8 text in double quotes. Supported escapes are `\n`, `\r`,
`\t`, `\"`, `\\`, `\{` and `\}`. Strings compare with `==` and `!=`; ordering
uses `String.compare`.

```morrow
fn main():
    println("tab\tquote\" backslash\\")
    println("line one\nline two")
    println("morrow" == "morrow")
```
```output
tab	quote" backslash\
line one
line two
true
```

#### Interpolation

Braces embed an expression in a string literal. The expression must evaluate to
an `Int`, `Float`, `Bool` or `String`; wrap other values in `show(...)`. Write
`\{` and `\}` for literal braces.

```morrow
fn main():
    let name = "Morrow"
    let items = [1, 2]
    println("Hello, {name}!")
    println("2 + 2 = {2 + 2}")
    println("items: {show(items)}, count: {List.len(items)}")
    println("braces: \{not interpolated\}")
```
```output
Hello, Morrow!
2 + 2 = 4
items: [1, 2], count: 2
braces: {not interpolated}
```

#### Triple-quoted strings

An ordinary string cannot contain a line break. A string delimited by `"""` can
span lines and keeps its content literally, including the line break after the
opening quotes and any leading indentation on each line. Interpolation works
inside it.

```morrow
fn main():
    let name = "Morrow"
    let banner = """
Hello, {name}
  indented line
"""
    print(banner)
```
```output

Hello, Morrow
  indented line
```

The first output line is empty because the literal begins with the line break
that follows `"""`.

String lengths and slice positions use UTF-8 bytes. The
[string examples](../../examples/string_ops.mr) show the core `String` functions.

### Unit

`()` is the unit value, the value of an expression that has nothing to return.
Its type is written `Unit` or `()`. `println`, an `if` without `else`, and a
`for` loop return Unit. Private functions infer their return type from their body;
`main` without an annotation returns Unit. Direct printing and interpolation of
Unit are rejected; `show(())` produces the string `"()"`.

```morrow
fn log(message: String):
    println("[log] {message}")

fn main():
    let result = log("started")
    let explicit: Unit = ()
    println("finished")
```
```output
[log] started
finished
```

### Tuples

A tuple groups a fixed number of values of possibly different types. Its type
lists the element types in parentheses. Access elements by position with `.0`,
`.1`, ... or destructure with a pattern.

```morrow
fn main():
    let pair: (Int, String) = (1, "one")
    println(pair.0)
    println(pair.1)
    let (number, word) = pair
    println("{number} is {word}")
    println((1, 2.5, true))
```
```output
1
one
1 is one
(1, 2.5, true)
```

### Ranges

`start..end` is a half-open range that excludes `end`; `start..=end` includes
it. Ranges have type `Range`, are iterated by `for`, and are lazy: no list is
built. `List.range(start, end)` produces a list when one is needed.

```morrow
fn main():
    for i in 1..4:
        print(i)
    println("")
    for i in 1..=4:
        print(i)
    println("")
    println(List.range(0, 3))
```
```output
123
1234
[0, 1, 2]
```

### Lists, options and results

List literals use brackets and hold elements of one type: `[1, 2, 3]`. `Some(x)`
and `None` build an `Option`; `Ok(x)` and `Err(e)` build a `Result`. These are
covered in [Collections](types.md#collections) and [Error handling](error-handling.md).

```morrow
fn main():
    println([1, 2, 3])
    println(Some(3))
    let missing: Option(Int) = None
    println(missing)
    let parsed: Result(Int, String) = Ok(1)
    println(parsed)
```
```output
[1, 2, 3]
Some(3)
None
Ok(1)
```

## Operators

### Arithmetic

`+`, `-`, `*`, `/`, `%` and `**` work on `Int`; all but `%` also work on
`Float`. Integer division truncates toward zero and `%` takes the sign of the
dividend. `**` is exponentiation and associates to the right.

```morrow
fn main():
    println(7 / 2)
    println(-7 / 2)
    println(7 % 3)
    println(-7 % 3)
    println(2 ** 10)
    println(2 ** 3 ** 2)
```
```output
3
-3
1
-1
1024
512
```

Integer division or remainder by zero and a negative integer exponent are
runtime faults that stop the program:

```output
morrow: runtime error: integer division by zero
```

### Comparison

`==` and `!=` compare `Int`, `Float`, `Bool` and `String` values. `<`, `<=`, `>`
and `>=` order `Int` and `Float`. Both operands must have the same type.
`x in list` tests list membership.

```morrow
fn main():
    println(1 == 1 and 2 != 3)
    println(3 < 4)
    println(2.5 >= 2.5)
    println("a" == "a")
    println(3 in [1, 2, 3])
```
```output
true
true
true
true
true
```

### Bitwise operators

Bitwise operators are spelled with three characters so they cannot be confused
with the boolean words: `&&&` (and), `|||` (or), `^^^` (exclusive or), `~~~`
(complement), `<<<` and `>>>` (shifts).

```morrow
fn main():
    println(6 &&& 3)
    println(6 ||| 3)
    println(6 ^^^ 3)
    println(~~~0)
    println(1 <<< 4)
    println(256 >>> 2)
```
```output
2
7
5
-1
16
64
```

### Precedence

From loosest to tightest binding. Operators on one row associate to the left
unless noted.

| Level | Operators | Notes |
| --- | --- | --- |
| 1 | `\|>` | pipe; see [Functions](../LANGUAGE_GUIDE.md#values-and-functions) |
| 2 | `or` | |
| 3 | `and` | |
| 4 | `..` `..=` | ranges; endpoints may not themselves be ranges |
| 5 | `\|\|\|` | |
| 6 | `^^^` | |
| 7 | `&&&` | |
| 8 | `==` `!=` | |
| 9 | `<` `<=` `>` `>=` `in` | |
| 10 | `<<<` `>>>` | |
| 11 | `+` `-` | |
| 12 | `*` `/` `%` | |
| 13 | `**` | right-associative |
| 14 | unary `-` `not` `~~~` | |
| 15 | `f(x)` `.field` `[index]` `?` | postfix |

Two consequences deserve attention. Unary operators bind tighter than every
binary operator, so `not a == b` parses as `(not a) == b`; write `not (a == b)`.
For the same reason `-n ** 2` is `(-n) ** 2`, and a literal `-2 ** 2` is `4`.

```morrow
fn main():
    println(1 + 2 * 3)
    println((1 + 2) * 3)
    println(not (1 == 2))
    let n = 2
    println(-n ** 2)
```
```output
7
9
true
4
```

## Checked integer arithmetic

`+`, `-` and `*` wrap silently at the 64-bit boundary, and `/` and `%` fault on
zero. When overflow or a zero divisor is a possibility the program must handle,
use the `Int.checked_*` functions. Each returns `Some(result)` for an exact
result and `None` for overflow, a zero divisor or `Int.min / -1`.

```morrow
fn main():
    println(9223372036854775807 + 1)
    println(Int.checked_add(9223372036854775807, 1))
    println(Int.checked_add(1, 2))
    println(Int.checked_div(1, 0))
    println(Int.checked_neg(-9223372036854775808))
```
```output
-9223372036854775808
None
Some(3)
None
None
```

The full set is `Int.checked_add`, `Int.checked_sub`, `Int.checked_mul`,
`Int.checked_div`, `Int.checked_rem` and `Int.checked_neg`. Match on the
`Option` to decide what an overflow means for the program:

```morrow
fn scale(value: Int, factor: Int) -> Int:
    match Int.checked_mul(value, factor):
        Some(product) -> product
        None -> 9223372036854775807

fn main():
    println(scale(value: 1000, factor: 1000))
    println(scale(value: 9223372036854775807, factor: 2))
```
```output
1000000
9223372036854775807
```

## Type annotations and inference

Local bindings are inferred from their values and rarely need annotations.
Function parameters and return types are the place where types are spelled out:
public functions must annotate every parameter and their return type. Private
functions may omit those annotations when the compiler can infer them from the
body, patterns and calls. For example, `fn twice(n): n * 2` infers an Int parameter
and result. Recursive or ambiguous definitions can need explicit annotations.
`main` keeps its separate entry contract: omitting its return annotation means Unit.
The built-in type names are:

| Type | Values |
| --- | --- |
| `Int`, `Float`, `Bool`, `String` | scalars |
| `Unit` or `()` | the unit value |
| `(A, B, ...)` | tuples |
| `List(a)` | immutable lists |
| `Option(a)` | `Some(value)` or `None` |
| `Result(a, e)` | `Ok(value)` or `Err(error)` |
| `Map(k, v)`, `Set(a)` | immutable maps and sets |
| `Range` | `start..end` values |
| `(A, B) -> C` or `fn(A, B) -> C` | function values |

Generic type arguments use parentheses, and a lowercase name in a type position
is a type variable: `List(a)` is a list of any element type. See
[Generic trait bounds](value-traits.md#bounds-on-generic-functions).

```morrow
fn first_or(items: List(a), fallback: a) -> a:
    match List.first(items):
        Some(item) -> item
        None -> fallback

fn main():
    let numbers: List(Int) = [4, 5]
    let words: List(String) = []
    let apply: (Int) -> Int = (n) -> n + 1
    println(first_or(numbers, 0))
    println(first_or(words, "none"))
    println(apply(41))
```
```output
4
none
42
```

An annotation that disagrees with the value is an error, and so is an `if` whose
branches produce different types:

```output
ifu2.mr:2:30: error: function return: let annotation: if branch: inferred expression type: expected Int, found String
      let v = if true: 1 else: "x"
```

User-defined records, tagged sums, newtypes, aliases and unions are the subject
of [Types](types.md).

## Naming conventions

The compiler attaches meaning to the case of some names, so conventions are
partly rules:

- Functions, parameters and local bindings use `snake_case`. In a pattern, a
  lowercase name is a binding and an uppercase name is a constructor, so a
  binding such as `Answer` would be read as a constructor inside `match`.
- Types and constructors use `PascalCase`: `Shape`, `Circle`, `Option`.
- Type variables are short lowercase names: `a`, `b`, `e`.
- Module paths are lowercase, dotted and mirror the file path: `geometry.shapes`.
- Labeled arguments reuse parameter names: `connect(host: "localhost", port: 80)`.

Reserved words cannot be used as names: `fn`, `let`, `const`, `comptime`, `if`,
`else`, `true`, `false`, `and`, `or`, `not`, `pub`, `type`, `match`, `return`,
`for`, `in`, `while`, `import`, `with`, `trait`, `impl`, `actor`, `receive`,
`spawn`, `send`, `where`, `do`, `defer`, `as`, `module`, `break`, `continue`,
`derive`, `newtype` and `after`. `while` is reserved although the language has no
`while` loop; iteration uses `for` and recursion.
