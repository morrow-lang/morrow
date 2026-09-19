+++
schema_version = 1
id = "01M2XHZ8XY2P5BN1CF70CN3AA9"
title = "SQLite-first SQL runtime backend with libsql-compatible API surface"
date = "2026-02-06"
status = "accepted"
tags = ["runtime", "storage"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Accepted
* **Decision**: I will implement `sql.open` and `sql.execute` on top of SQLite (`sqlite3`) first, while keeping Fern's SQL API surface stable so we can layer or swap to libsql later without changing Fern source signatures.
* **Context**: The runtime previously returned placeholder `Err(FERN_ERR_IO)` for all SQL calls, which created a major product-surface gap even though `sql.*` type signatures were stabilized. Integrating full libsql transport/features immediately would add substantial dependency and packaging complexity. A SQLite-first backend delivers concrete local database behavior now and keeps progress aligned with current Gate C stabilization priorities.
* **Consequences**: `fern_sql_open` now returns opaque handle ids for opened SQLite connections and `fern_sql_execute` returns rows affected via `sqlite3_changes()`. Runtime/link paths now include `sqlite3` linkage, and runtime-surface tests now cover successful SQL create/insert flows plus invalid-handle errors. HTTP remains placeholder-backed until its runtime backend lands.
