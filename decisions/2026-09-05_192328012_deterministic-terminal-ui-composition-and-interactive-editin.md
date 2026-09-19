+++
schema_version = 1
id = "01M2XHZ8WCQS8T586CTKJFH314"
title = "Deterministic terminal UI composition and interactive editing"
date = "2026-09-05"
status = "accepted"
tags = ["testing", "shell"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Decision**: I will reuse vendored linenoise for interactive prompt editing, preserve plain line reads for pipes, compose immutable trees with `new`, `add`, and `render`, expose deterministic log formatters, and emit cursor controls only on terminals.
* **Context**: Existing terminal modules need structured output and usable editing without another dependency or timing-dependent tests. The `/decision` skill is unavailable; this entry follows the existing format directly.
* **Consequences**: PTY tests cover interactive behavior and deterministic output fixtures cover trees/logs. Redirected output remains suitable for scripts.
