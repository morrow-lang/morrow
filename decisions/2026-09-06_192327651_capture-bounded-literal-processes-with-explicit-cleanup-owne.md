+++
schema_version = 1
id = "01M2XHZ8H3REB4VSX442VMWKZ7"
title = "Capture bounded literal processes with explicit cleanup ownership"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for native execution and bootstrap workflows
* **Decision**: I will expose `System.exec_args_bounded(List(String), Int, Int) -> Result((Int, String, String), Int)` through a shared heap Result ABI. Normal exit statuses, including 127, remain successful captures; configuration, spawn, deadline, size, IO, text and signal failures have separate stable Int codes.
* **Context**: Developer tooling needs literal argv, independent stdout/stderr and explicit resource limits. C and Rust tuple representations differ, so Rust adapts only the successful native tuple after checking the Result tag. Full-width source arguments are required by decision 86.
* **Consequences**: [The execution contract](../docs/PROCESS_EXECUTION.md) specifies argument/PATH/text limits, 1–600,000 ms deadlines, independent 0–16 MiB streams, stdin EOF and preserved caller descriptors/signals. Explicit bounded PATH search uses `posix_spawn`, because macOS `posix_spawnp` can run a shell on ENOEXEC. Every child owns a private process group and retains its unreaped identity until cleanup; escaped descendants are not contained, and OS cleanup can extend wall-clock time. Only confirmed exited-child-only Darwin groups allow the documented conservative EPERM exception. Embedding excludes competing reapers and concurrent signal/environment policy mutation. Debug/release/sanitizer and source-native tests cover failure paths and resource boundaries. Legacy process APIs remain compatibility surfaces. The unavailable `/decision` skill is replaced by this established format.
