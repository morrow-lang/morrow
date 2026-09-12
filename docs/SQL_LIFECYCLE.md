# SQL connection lifecycle

Fern exposes `sql.open(path)`, `sql.execute(handle, query)`, and
`sql.close(handle)`. Each returns `Result(Int, Int)`. Connections are backed by
local SQLite through rusqlite; remote libSQL connections and row-query APIs remain planned.

`sql.open` returns a positive opaque handle. Treat it as a process-local token,
not an address or a predictable index. `sql.execute` preserves its existing
`sqlite3_changes` result. `sql.close` returns `Ok(0)` only when SQLite closes the
connection. A successful close rolls back any unfinished transaction and releases
its native resources and locks. Commit explicitly before closing to retain a
transaction's changes.

Invalid, negative, unknown, or already-closed handles return `Err(3)` from execute
and close. If SQLite refuses a close, the same error is returned and the connection
remains live for a retry. Closed IDs never become valid again, even after their
storage slots are reused. Closing one handle does not invalidate other handles.

At most 256 connections may remain live in one process. Open returns `Err(4)`
before opening or creating a database when this quota is full. Closing immediately
releases one slot. Repeated sequential open/close operations do not consume quota
or grow registry storage. IDs increase monotonically; reaching `INT64_MAX` also
returns `Err(4)` rather than overflowing or reusing an ID. Invalid paths return
`Err(3)` and consume no slot. These bounds cover handles, not SQLite database size,
query execution time, or memory allocated internally by SQLite.

```fern
fn report(result: Result(Int, Int)) -> Int:
    match result:
        Ok(value) -> value
        Err(error) -> -error

fn main():
    match sql.open(":memory:"):
        Ok(db) ->
            println(report(sql.execute(db, "create table example (value integer)")))
            println(report(sql.close(db)))
        Err(error) -> println(error)
```

The example reports execution and close results and closes even if execution fails. A bare early-return
`?` after opening does not automatically close a connection. Native process exit
remains the final cleanup boundary for connections a program leaves open.

The Rust REPL intentionally reports SQL as available only in native programs.
This change does not add an interpreter database backend or pretend that an
interactive close succeeded.

Rust runtime tests cover statement execution, double-close, stale-handle isolation,
the 256/257 live boundary, capacity recovery and monotonic handle identities.
Native source fixtures exercise the public calls and full-width Result transport.

```sh
cargo test -p fern-runtime services::sql
cargo test -p fern-runtime --release services::sql
cargo xtask native sql
```
