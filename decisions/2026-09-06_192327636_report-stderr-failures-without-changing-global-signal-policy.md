+++
schema_version = 1
id = "01M2XHZ8GMYGKXZV2H2EZH7JV4"
title = "Report stderr failures without changing global signal policy"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for native developer tooling
* **Decision**: I will expose `System.write_stderr(String) -> Result(Unit, Int)` as exact UTF-8 output without an inserted newline. Validate at most 16 MiB before writing; return stable errors for invalid text, size and IO. An output failure does not implicitly replace the caller's primary exit status.
* **Context**: The Fern checker must emit argument errors on stderr, and a closed pipe must produce a handled error instead of terminating the process. Reopening descriptors, toggling shared flags or changing process-global SIGPIPE handlers would interfere with embedding callers.
* **Consequences**: Writes use 16 KiB chunks and at most 65,536 attempts, including EINTR. Only the calling thread masks SIGPIPE; its prior mask and preexisting pending signal are preserved. After EPIPE, only a newly pending signal is consumed before restoration. Embedding excludes competing consumers, disposition changes and simultaneous SIGPIPE injection into that thread. Partial output can precede an error, and blocking kernel writes have no hard deadline. Empty text succeeds without descriptor access. Native heap Result needs no Rust adapter; C also canonicalizes Unit annotations. REPL native effects remain explicitly unavailable. [The API contract](../docs/PROCESS_EXECUTION.md#standard-error-output) records these limits. The unavailable `/decision` skill is replaced by this established format.
