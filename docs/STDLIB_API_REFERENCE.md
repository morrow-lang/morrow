# Fern Standard Library API Reference (Gate C)

Last updated: 2026-09-12

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

The following signatures describe the legacy C frontend. The Rust frontend now
uses opaque values/errors and immutable builders; see [the Rust JSON API](JSON_RUST_API.md).
Its `parse` returns `Result(json.Value, json.Error)` and `stringify` accepts a Value.
The `Json` compatibility spelling follows the selected frontend.

```fern
json.parse(text: String) -> Result(String, Int)
json.stringify(text: String) -> Result(String, Int)
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

## Runtime Readiness (2026-02-06)

1. `fs`, `json`, and `actors` have concrete runtime behavior covered by regression tests.
2. `sql` signatures are stable and backed by a concrete SQLite runtime (`sql.open`, `sql.execute`).
3. `http` signatures are stable and backed by a civetweb client runtime for HTTP/HTTPS requests (`http.get`, `http.post`), returning `Err(FERN_ERR_IO)` for invalid URLs/network failures/non-`2xx` responses.
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

These API contracts are enforced by type-checker tests:

1. `test_check_fs_api_signatures`
2. `test_check_json_api_signatures`
3. `test_check_http_api_signatures`
4. `test_check_sql_api_signatures`
5. `test_check_actors_api_signatures`
6. `test_check_file_alias_api_signatures`

See `tests/test_checker.c`.
