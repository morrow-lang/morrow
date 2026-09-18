# Error handling

Morrow represents failure and absence as ordinary values: `Result(a, e)` and
`Option(a)`. This chapter covers matching on them, the `?` operator, `with` blocks,
`defer`, custom error types, the standard library's error codes and the compile-time
rule that every `Result` must be handled.

There are no exceptions and no `null`. A function that can fail says so in its return
type, and a caller that receives a `Result` cannot forget it: the compiler rejects a
program that produces a `Result` and never inspects it.

## Result and Option

`Result(a, e)` is `Ok(value)` with a value of type `a` or `Err(error)` with an error of
type `e`. `Option(a)` is `Some(value)` or `None`. Both are tagged sums, so `match`
destructures them, and both are listed among the [built-in types](types.md#option-and-result).

```morrow
fn parse(text: String) -> Result(Int, String):
    match Int.parse(text):
        Some(n) -> Ok(n)
        None -> Err("not a number: {text}")

fn main():
    match parse("12"):
        Ok(n) -> println(n * 2)
        Err(message) -> println(message)
    match parse("twelve"):
        Ok(n) -> println(n * 2)
        Err(message) -> println(message)
```
```output
24
not a number: twelve
```

`Int.parse` returns `Option(Int)`: malformed text and numbers outside the signed
64-bit range both yield `None`, without an error payload. `parse` turns that absence into an error message, which is the
usual way to move from `Option` to `Result`. Use `Option` when a value may be missing
and there is nothing more to say; use `Result` when the caller needs to know why.

## Results must be handled

Every `Result` a function produces carries an obligation: before the function returns,
the program must have matched it, propagated it with `?`, returned it to the caller or
passed it to a function that does one of those. Dropping the value is a compile error.
`bin/morrow check report.mr` on a program that calls `fs.read` and ignores the value
prints:

```text
report.mr:2:5: error: Result value must be handled; bind, return, or match it
      fs.read("missing.txt")
```

Binding it to a name that is never used does not help:

```text
binding.mr:2:19: error: Result value must be handled; this Result binding is never used
      let content = fs.read("missing.txt")
```

What counts as handling is the tag being inspected:

- An exhaustive `match` with `Ok` and `Err` arms, or a `match` on a collection or
  record that reaches every contained `Result`.
- `?`, which handles the outer tag and hands the `Err` payload to the caller.
- Returning the `Result`, or a collection that contains it, to the caller.
- `Result.is_ok`, `Result.is_err`, `Result.unwrap_or` and the other
  [combinators](#combinators), and printing through `Show`, which matches both cases.

What does not count: a `match` whose only arm is `_`, reading `List.len` of a list of
results, or passing the value to a function that never inspects its tags. A `Result` nested
inside an `Ok` payload or inside a list element carries its own obligation. Closures
cannot capture a value that still has an obligation, so handle it before writing the
lambda:

```text
capture.mr:3:23: error: capturing Result-bearing values in closures is currently unsupported; handle the Result before capturing
      let later = () -> result
```

The full proof rules, including collections, callbacks and recursive types, are in
[Result handling](../RESULT_HANDLING.md).

## The ? operator

`expression?` unwraps an `Ok` and continues, or returns the `Err` from the enclosing
function immediately. It is the common way to propagate errors.

```morrow
fn parse(text: String) -> Result(Int, String):
    match Int.parse(text):
        Some(n) -> Ok(n)
        None -> Err("not a number: {text}")

fn add(left: String, right: String) -> Result(Int, String):
    let a = parse(left)?
    let b = parse(right)?
    Ok(a + b)

fn main():
    println(add(left: "1", right: "2"))
    println(add(left: "1", right: "two"))
```
```output
Ok(3)
Err("not a number: two")
```

Three rules govern `?`:

1. The enclosing function must return a `Result`. In a function returning `()` or
   `Int` the compiler reports `? requires a function returning Result`.
2. The operand's error type must equal the function's error type exactly. Propagating
   a `Result(Int, String)` from a function returning `Result(Int, Int)` fails:

   ```text
   mismatch.mr:8:18: error: function return: let annotation: ? error type: expected Int, found String
         let number = parse(text)?
   ```

   Convert the error first, either with a `match` that returns a different `Err`, or
   by giving your function an error type that covers both cases. See
   [Custom error types](#custom-error-types).
3. The operand must be a `Result`. `?` does not apply to `Option`:

   ```text
   option.mr:2:17: error: function return: let annotation: ? operand must be Result: expected Result with unresolved payload types, found Option(Int)
         let value = List.first(items)?
   ```

`?` works as a statement too: `fs.append(path, line)?` discards the `Ok` payload and
propagates any `Err`. An explicit `return Err(...)` leaves the function the same way:

```morrow
fn parse(text: String) -> Result(Int, String):
    match Int.parse(text):
        Some(n) -> Ok(n)
        None -> Err("not a number: {text}")

fn half(text: String) -> Result(Int, String):
    let n = parse(text)?
    if n % 2 != 0:
        return Err("{n} is odd")
    Ok(n / 2)

fn main():
    println(half("8"))
    println(half("7"))
    println(half("x"))
```
```output
Ok(4)
Err("7 is odd")
Err("not a number: x")
```

`main` itself may return `Result((), e)` with a concrete error type `e`, which lets it use `?`. When `main` returns `Err`,
the program prints `morrow: main returned Err` on standard error and exits with status 1.

```morrow
fn parse(text: String) -> Result(Int, String):
    match Int.parse(text):
        Some(n) -> Ok(n)
        None -> Err("not a number: {text}")

fn main() -> Result((), String):
    let value = parse("41")?
    println(value + 1)
    Ok(())
```
```output
42
```

## with blocks

A `with` block binds several `Result` values in sequence with `<-`, runs the `do` body
when all of them are `Ok`, and otherwise jumps to the `else` arms with the first
`Err`. Later bindings may use earlier ones. `<-` is only valid inside `with`.

```morrow
type AuthFailure:
    Denied(user: String)

type LoadFailure:
    Missing(path: String)

fn authenticate(user: String) -> Result(Int, AuthFailure):
    if user == "ada": Ok(1) else: Err(Denied(user))

fn load(id: Int) -> Result(String, LoadFailure):
    if id == 1: Ok("profile") else: Err(Missing("/profiles/{id}"))

fn fetch(user: String) -> String:
    with
        id <- authenticate(user),
        profile <- load(id)
    do
        "loaded {profile} for {user}"
    else
        Err(Denied(name)) -> "access denied for {name}"
        Err(Missing(path)) -> "nothing at {path}"

fn main():
    println(fetch("ada"))
    println(fetch("bob"))
```
```output
loaded profile for ada
access denied for bob
```

The bound results may have different error types; the `else` arms together must cover
every error that any binding can produce, and every arm must have the same type as the
`do` body. Arms accept guards. Without an `else`, the enclosing function must return a
`Result` and the first `Err` is returned from it, which makes `with` a grouped form of
`?`:

```morrow
fn parse(text: String) -> Result(Int, String):
    match Int.parse(text):
        Some(n) -> Ok(n)
        None -> Err("not a number: {text}")

fn sum(left: String, right: String) -> Result(Int, String):
    with
        a <- parse(left),
        b <- parse(right)
    do
        Ok(a + b)

fn main():
    println(sum(left: "2", right: "3"))
    println(sum(left: "2", right: "x"))
```
```output
Ok(5)
Err("not a number: x")
```

Use `?` when errors should pass through unchanged and `with` when several related
operations share one place that decides what each failure means.

## Custom error types

Any type can be the `e` in `Result(a, e)`. A tagged sum with one variant per failure
gives callers something to match on and lets one function combine errors from several
sources. Converting a library error into your own type is a `match` that returns early.

```morrow
type AppError:
    Io(code: Int)
    Parse(text: String)

fn parse(text: String) -> Result(Int, AppError):
    match Int.parse(String.trim(text)):
        Some(n) -> Ok(n)
        None -> Err(Parse(text))

fn read_number(path: String) -> Result(Int, AppError):
    let text = match fs.read(path):
        Ok(content) -> content
        Err(code) -> return Err(Io(code))
    parse(text)

fn report(path: String):
    match read_number(path):
        Ok(n) -> println(n + 1)
        Err(Io(code)) -> println("io error {code}")
        Err(Parse(text)) -> println("bad number: {text}")

fn main():
    match fs.write("/tmp/morrow-guide-number.txt", "41"):
        Ok(_) -> ()
        Err(code) -> println("setup failed: {code}")
    report("/tmp/morrow-guide-number.txt")
    report("/tmp/morrow-guide-absent.txt")
```
```output
42
io error 1
```

Because `read_number` ends with `parse(text)`, its `Result` is returned to the caller
directly; the obligation moves with it. Deriving `Show` and `Eq` on an error type is
useful for logging and tests; see [Traits](value-traits.md#deriving).

## Standard library error codes

Runtime-facing modules report errors as `Int` codes. The codes are small and specific
to each module.

| Call | Success | Error codes |
| --- | --- | --- |
| `fs.read(path)` | `Ok(text)` | 1 open for reading failed, 3 IO/invalid UTF-8/over 16 MiB |
| `fs.write`, `fs.append` | `Ok(bytes)` | 2 open for writing failed, 3 IO or size failure |
| `fs.delete`, `fs.size` | `Ok(Int)` | 1 missing, 2 permission, 3 other |
| `fs.list_dir(path)` | `Ok(entries)` | 1 missing, 2 permission, 5 not a directory, 3 other |
| `http.get`, `http.post` | `Ok(body)` for 2xx | 3 for invalid URL, transport, redirect or non-2xx |
| `sql.open`, `sql.execute`, `sql.close` | `Ok(Int)` | 3 invalid input/handle or SQLite failure; `sql.open` also returns 4 when connection capacity is exhausted |

```morrow
fn main():
    match fs.read("/tmp/morrow-guide-does-not-exist.txt"):
        Ok(text) -> println(text)
        Err(code) -> println("read failed with code {code}")
    match fs.list_dir("/etc/hosts"):
        Ok(entries) -> println(List.len(entries))
        Err(code) -> println("list failed with code {code}")
```
```output
read failed with code 1
list failed with code 5
```

The JSON module uses an opaque `json.Error` instead of an integer. `json.error_code`,
`json.error_offset` and `json.error_message` read it; the offset is the byte position
in the input, or -1 when the error did not come from parsing.

```morrow
fn report(result: Result(json.Value, json.Error)):
    match result:
        Ok(_) -> println("parsed")
        Err(error) ->
            println(json.error_code(error))
            println(json.error_offset(error))
            println(json.error_message(error))

fn main():
    report(json.parse("[1, 2"))
    report(json.parse("[1]"))
```
```output
1
5
invalid JSON syntax
parsed
```

Current signatures are listed in the [standard library reference](../STDLIB_API_REFERENCE.md)
and described in the [JSON API](../JSON_RUST_API.md),
[file IO](../FILE_TEXT_IO.md) and [process execution](../PROCESS_EXECUTION.md) guides.

## Combinators

The `Result` and `Option` modules provide functions for the common shapes of handling
without a `match`. Each of them counts as handling the value it receives.

| Function | Result |
| --- | --- |
| `Result.map(result, f)` | applies `f` to an `Ok` payload |
| `Result.and_then(result, f)` | applies `f`, which returns a `Result`, to an `Ok` payload |
| `Result.unwrap_or(result, default)` | the `Ok` payload or `default` |
| `Result.unwrap_or_else(result, f)` | the `Ok` payload or `f(error)` |
| `Result.is_ok(result)`, `Result.is_err(result)` | the tag as a `Bool` |
| `Option.map(option, f)` | applies `f` to a `Some` payload |
| `Option.unwrap_or(option, default)` | the `Some` payload or `default` |
| `Option.is_some(option)`, `Option.is_none(option)` | the tag as a `Bool` |

```morrow
fn parse(text: String) -> Result(Int, String):
    match Int.parse(text):
        Some(n) -> Ok(n)
        None -> Err("not a number: {text}")

fn main():
    println(Result.map(parse("4"), (n) -> n * 10))
    println(Result.and_then(parse("2"), (n) -> if n > 3: Ok(n) else: Err("too small")))
    println(Result.unwrap_or(parse("x"), -1))
    println(Result.unwrap_or_else(parse("x"), (message) -> String.len(message)))
    println(Result.is_ok(parse("1")))
    println(Option.map(Int.parse("7"), (n) -> n + 1))
    println(Option.unwrap_or(Int.parse("?"), 0))
```
```output
Ok(40)
Err("too small")
-1
15
true
Some(8)
0
```

There is no `unwrap` that faults on `Err` or `None`. The closest operations are
`unwrap_or` with a default and `?` with propagation.

## Collecting many results

Producing a list of results and then propagating each one with `?` is rejected: after
the first `Err` the remaining elements would be abandoned unhandled. Fold the inputs
into a single `Result` instead. After the first failure, the fold carries that
error through the remaining inputs without parsing them:

```morrow
fn parse(text: String) -> Result(Int, String):
    match Int.parse(text):
        Some(n) -> Ok(n)
        None -> Err("not a number: {text}")

fn parse_all(texts: List(String)) -> Result(List(Int), String):
    List.fold(texts, Ok([]), (acc, text) ->
        match acc:
            Ok(numbers) ->
                match parse(text):
                    Ok(n) -> Ok(List.push(numbers, n))
                    Err(e) -> Err(e)
            Err(e) -> Err(e)
    )

fn main():
    println(parse_all(["1", "2", "3"]))
    println(parse_all(["1", "x", "3"]))
```
```output
Ok([1, 2, 3])
Err("not a number: x")
```

A `for` loop that matches every element of a `List(Result(a, e))` also handles all of
them; a loop that exits early, or a `List.find`, does not.

## Cleanup with defer

`defer expression` schedules the expression to run when the enclosing function exits,
on every path: normal return, `return`, `?` and `with` error exits. Several `defer`
statements run in reverse order of registration. The deferred expression must have type
`()`, so a cleanup that itself returns a `Result` must be wrapped in a function that
handles it.

```morrow
fn work(fail: Bool) -> Result(Int, String):
    defer println("cleanup 1")
    defer println("cleanup 2")
    println("working")
    if fail: Err("failed")? else: Ok(0)?
    println("finished")
    Ok(1)

fn main():
    println(work(fail: false))
    println(work(fail: true))
```
```output
working
finished
cleanup 2
cleanup 1
Ok(1)
working
cleanup 2
cleanup 1
Err("failed")
```

A typical use removes a temporary file whether or not the writes succeed:

```morrow
fn cleanup(path: String):
    match fs.delete(path):
        Ok(_) -> println("removed {path}")
        Err(code) -> println("could not remove {path}: {code}")

fn write_report(path: String, lines: List(String)) -> Result(Int, Int):
    fs.write(path, "")?
    defer cleanup(path)
    for line in lines:
        fs.append(path, line)?
    fs.size(path)

fn main():
    println(write_report("/tmp/morrow-guide-report.txt", ["one\n", "two\n"]))
    println(write_report("/tmp/morrow-guide-no-such-dir/report.txt", ["one\n"]))
```
```output
removed /tmp/morrow-guide-report.txt
Ok(8)
Err(2)
```

The second call fails at `fs.write` before the `defer` is registered, so nothing is
removed. Writing `defer fs.delete(path)` directly is rejected with `defer requires Unit
cleanup: expression type: expected (), found Result(Int, Int)`. A deferred handler may
also be the thing that handles a `Result` produced earlier in the function; the
compiler accepts that as long as the handler is registered on every path that produced
the value.

## No exceptions

Morrow has no exception syntax. Expected failures are represented as `Result` values.
The remaining failures are runtime faults: integer division by zero, `List.head` of an
empty list, exceeding a resource limit. A fault runs the deferred cleanup of the active
functions, prints a diagnostic on standard error and ends the process with exit status
1. It cannot be caught.

```morrow
fn main():
    defer println("deferred cleanup")
    let empty: List(Int) = []
    println(List.head(empty))
```
```output
deferred cleanup
morrow: runtime error: head of empty list
```

Prefer the non-faulting forms where they exist: `List.first` and `List.at` return
`Option`, and `Int.checked_div` returns `None` for a zero divisor. Actors have their
own failure and restart model, described in [Actors](../ACTOR_RUNTIME.md).

## Limits of the handling proof

The compiler proves handling within a bounded budget and rejects what it cannot prove.
Aliases of a `Result` share one obligation, so handling either alias is enough.
Replacing a map value or consuming an accumulator without handling the old `Result`
inside it is rejected. Recursive functions over trees that store `Result` values are
supported when each call receives a strict descendant of its input. When the proof
gives up, the diagnostic says so; it never grants handling credit silently. The exact
contracts and the budgets are documented in [Result handling](../RESULT_HANDLING.md).
