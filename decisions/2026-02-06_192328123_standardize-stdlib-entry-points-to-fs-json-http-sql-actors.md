+++
schema_version = 1
id = "01M2XHZ8ZVZFNQ23Y3FKHGTWGA"
title = "Standardize stdlib entry points to fs/json/http/sql/actors"
date = "2026-02-06"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Accepted
* **Decision**: I will stabilize `fs`, `json`, `http`, `sql`, and `actors` as Fern's top-level stdlib entry points, while keeping `File.*` as a compatibility alias during migration.
* **Context**: Gate C requires a predictable product surface before deeper runtime work. The compiler already exposed mixed module names (`File`, `System`, `Regex`, `Tui.*`) and lacked a consistent contract for the next stdlib modules. We need a clear API front door now so future runtime implementation work can proceed without renaming churn.
* **Consequences**: Checker/codegen now recognize the stabilized entry points and are covered by regression tests. `File.*` remains supported for compatibility and will follow the formal deprecation policy if retired. Runtime semantics for the new module surfaces can evolve behind these stable names.
