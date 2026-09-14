# Fern Standard Library API Reference (Gate C)

Last updated: 2026-09-14

This file is the canonical signature reference for the Gate C core standard-library modules.
It complements `docs/COMPATIBILITY_POLICY.md` with concrete function-level contracts.

## Scope

The APIs below are treated as stable in Gate C:

1. `fs`
2. `json`
3. `http`
4. `sql`
5. `actors`

Compatibility alias:

1. `File.*` is a compatibility alias for `fs.*`.

Canonical naming in new code/docs:

1. Prefer `fs.*` over `File.*`.
2. Keep service modules lowercase: `json`, `http`, `sql`, `actors`.
3. Keep core utility modules in PascalCase: `String`, `List`, `System`, `Regex`, `Result`, `Option`, `Tui.*`.

## Result Type Syntax and Error Code Type

Current runtime-facing APIs use `Int` for error codes.

1. Fern generic type syntax is `Result(T, E)` (parentheses, not brackets).
2. `Result(T, Int)` means `Err(Int)` where integer values map to runtime error constants.

## Module Signatures

### Regex capture positions

The Rust frontend exposes `Regex.captures(text, pattern)` as a list of
`(start: Int, end: Int, text: String)` tuples. Entry zero is the full match;
the shared runtime returns up to nine numbered subgroups in pattern order.
An optional group that does not participate keeps its slot with `(-1, -1, "")`.
A participating empty group has equal nonnegative byte offsets and empty text.
Invalid patterns and searches with no match return an empty list. Patterns use
the runtime's POSIX extended regular expression syntax.

### `String` decimal predicate

```fern
String.is_decimal(text: String) -> Bool
```

Nonempty Unicode 16.0.0 decimal-digit text is true. Empty text, signs and other
numeric categories are false. The intentional `str_is_decimal` alias has the same
signature. See [the classifier contract](STRING_DECIMAL.md) for the 16 MiB native
limit, generated data provenance and interactive work budgets.

### Portable string helpers and Sets

```fern
String.compare(left: String, right: String) -> Int
String.join(parts: List(String), separator: String) -> String
```

These runtime APIs take positional arguments. `String.compare` returns -1, 0 or 1
for lexical UTF-8 ordering, without locale collation or Unicode normalization.
`String.join` inserts the separator between elements; an empty list produces an
empty string. Both execute in native, REPL and WASM programs under their target's
string and resource limits. The built-in value traits use these portable helpers.

### Non-faulting positional access and integer parsing

```fern
List.at(items: List(a), index: Int) -> Option(a)
List.first(items: List(a)) -> Option(a)
List.last(items: List(a)) -> Option(a)
List.take(items: List(a), count: Int) -> List(a)
List.drop(items: List(a), count: Int) -> List(a)
Int.parse(text: String) -> Option(Int)
```

`List.get` and `List.head` fault on invalid positions; these APIs never fault.
`List.at` returns `None` for negative or out-of-range indexes, and `List.first`
and `List.last` return `None` for empty lists. Every `Some` payload is the
full-width element word. `List.take` and `List.drop` clamp the count into
`0..=List.len(items)`: negative counts take nothing or drop nothing, oversized
counts take everything or drop everything, and the source list is never aliased.
`Int.parse` accepts exactly `[+-]?[0-9]+` within the signed 64-bit range; leading
or trailing whitespace, separators, radix prefixes, exponents, non-ASCII digits
and out-of-range values yield `None`. Native, REPL and comptime evaluation share
these contracts.

### List utilities

```fern
List.sort(items: List(a)) -> List(a)
List.zip(left: List(a), right: List(b)) -> List((a, b))
List.range(start: Int, end: Int) -> List(Int)
List.sum(items: List(Int)) -> Int
```

`List.sort` returns a new ascending list and requires `Int`, `Float`, `Bool` or
`String` elements (or newtypes over them); the checker rejects other element types.
Strings order by UTF-8 bytes like `String.compare`; Floats use IEEE total order, so
`-0.0` sorts before `0.0` and NaN sorts last. `List.zip` pairs positionally and
stops at the shorter list. `List.range` is half-open: `List.range(0, 3)` is
`[0, 1, 2]` and an end at or below the start is empty. `List.sum` uses the
language's wrapping addition and returns `0` for an empty list. Ranges and zips
above 16,777,216 elements fault with the list size limit.

### Checked integer arithmetic

```fern
Int.checked_add(left: Int, right: Int) -> Option(Int)
Int.checked_sub(left: Int, right: Int) -> Option(Int)
Int.checked_mul(left: Int, right: Int) -> Option(Int)
Int.checked_div(left: Int, right: Int) -> Option(Int)
Int.checked_rem(left: Int, right: Int) -> Option(Int)
Int.checked_neg(value: Int) -> Option(Int)
```

The operators `+ - *` wrap and `/ %` fault on zero (Decision 55). These functions
return `None` instead for overflow, a zero divisor and `Int.min / -1`; every `Some`
payload is the exact 64-bit result.

### Printing structured values

`print` and `println` accept `Int`, `Float`, `Bool` and `String` directly. Any
other value type is printed through its `Show` implementation, so lists, options,
results, tuples, maps and types declared with `derive(Show)` print without an
explicit `show(...)` call. A type without `Show` is rejected at check time with a
hint to derive it. `Unit` and function values remain rejected.

[`Set(a)`](SETS.md) adds thirteen immutable collection operations with a distinct
nominal identity, insertion-ordered iteration and membership equality. Its key
types and complexity guarantees follow the documented Set contract.

### `fs`

```fern
fs.read(path: String) -> Result(String, Int)
fs.write(path: String, content: String) -> Result(Int, Int)
fs.append(path: String, content: String) -> Result(Int, Int)
fs.exists(path: String) -> Bool
fs.delete(path: String) -> Result(Int, Int)
fs.size(path: String) -> Result(Int, Int)
fs.list_dir(path: String) -> Result(List(String), Int)
```

Directory listing excludes `.` and `..`; order is unspecified. Empty directories
return `Ok([])`. Errors use codes 1 (missing), 2 (permission), 5 (not a directory),
and 3 (other IO or more than 1,048,576 entries). This signature is an explicit
unreleased migration change; see the compatibility policy and Decision 50.

### `json`

The default Rust compiler uses opaque values/errors and immutable builders.
`Json` and `json` identify the same operations and types. The full dynamic API,
resource limits and typed codecs are documented in [the JSON API](JSON_RUST_API.md).

```fern
json.parse(text: String) -> Result(json.Value, json.Error)
json.stringify(value: json.Value) -> Result(String, json.Error)
```

### `http`

```fern
http.get(url: String) -> Result(String, Int)
http.post(url: String, body: String) -> Result(String, Int)
```

### `sql`

```fern
sql.open(path: String) -> Result(Int, Int)
sql.execute(handle: Int, query: String) -> Result(Int, Int)
sql.close(handle: Int) -> Result(Int, Int)
```

`sql.close` returns `Ok(0)` after releasing the connection. Invalid or already
closed handles return `Err(3)`. At most 256 connections may remain open; attempting
to exceed this quota returns `Err(4)` before opening a database. Closing releases
quota immediately and never makes an old handle valid again. See
[SQL connection lifecycle](SQL_LIFECYCLE.md) for transaction behavior and limits.

### `actors`

```fern
actors.start(name: String) -> Int
actors.post(actor_id: Int, message: String) -> Result(Int, Int)
actors.next(actor_id: Int) -> Result(String, Int)
```

## Runtime implementation

1. `fs`, `json`, and `actors` have concrete runtime behavior covered by regression tests.
2. `sql` signatures are stable and backed by a concrete SQLite runtime (`sql.open`, `sql.execute`).
3. `http` uses ureq/rustls for HTTP/HTTPS with certificate verification, a 30-second deadline and a 16 MiB body limit. Invalid URLs, redirects, transport errors, invalid text and non-2xx responses return `Err(3)`.
4. `File.*` is maintained for compatibility and maps to the same runtime surface as `fs.*`.

### `File` compatibility alias

```fern
File.read(path: String) -> Result(String, Int)
File.write(path: String, content: String) -> Result(Int, Int)
File.append(path: String, content: String) -> Result(Int, Int)
File.exists(path: String) -> Bool
File.delete(path: String) -> Result(Int, Int)
File.size(path: String) -> Result(Int, Int)
```

## Stability Enforcement

Rust registry/checker tests verify signatures and alias identities. Independent
native output fixtures, runtime tests and `cargo xtask compatibility` exercise
execution and rejection before output mutation. See the
[workspace acceptance](RUST_WORKSPACE.md) for the exact platform verification.
