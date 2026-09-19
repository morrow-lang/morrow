+++
schema_version = 1
id = "01M2XHZ96SY5PS19CN2XJ05BRF"
title = "defer statement for resource cleanup"
date = "2026-01-27"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Adopted
* **Decision**: I will add a `defer` statement (from Zig) for guaranteed resource cleanup.
* **Context**: Resource cleanup (closing files, freeing locks, etc.) must be reliable even when errors occur. Considered: (1) try/finally blocks - verbose and easy to forget, (2) RAII/destructors - implicit, hard to see cleanup order, (3) `defer` statement - explicit, clear cleanup order (reverse of declaration), always runs on scope exit. Defer makes cleanup visible and guaranteed without ceremony.
* **Consequences**: The compiler must track defer statements and ensure they execute on all exit paths (return, error, normal). Deferred calls execute in reverse order of declaration.
