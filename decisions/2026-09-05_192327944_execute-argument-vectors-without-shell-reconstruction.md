+++
schema_version = 1
id = "01M2XHZ8T8ZDE5K4NDP3R0NS5Y"
title = "Execute argument vectors without shell reconstruction"
date = "2026-09-05"
status = "accepted"
tags = ["shell"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Decision**: I will implement `System.exec_args` with `posix_spawnp` and literal argv, capturing stdout/stderr in private unlinked files.
* **Context**: The previous runtime reconstructed a shell command and underallocated its buffer when escaping single quotes. It contradicted the documented no-shell API and could corrupt memory.
* **Consequences**: Empty or missing commands and signal termination produce exit code -1; normal exit statuses and both streams are retained. Argument bytes never become shell syntax. Temporary capture descriptors are normalized above standard streams and closed after waiting; the separate `System.exec` API retains explicit shell semantics.
