+++
schema_version = 1
id = "01M2XHZ8EZ57TEMFEXDRT0B6PC"
title = "Check formatting without modifying source files"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for the Rust CLI
* **Decision**: I will add `fern-rs fmt --check source.fn`, accepting the flag before or after the source, with silent exit0 for canonical text and exit1 plus a source diagnostic for formatting drift. Check mode performs no writes or temporary-file creation.
* **Context**: CI needs to enforce the same formatting users apply locally without changing their checkout. Reusing the bounded syntax formatter preserves one canonical output instead of maintaining a separate style approximation.
* **Consequences**: Existing `fmt source.fn` still writes atomically and preserves permissions. Check mode preserves bytes, modification time, inode and symlink identity on success, drift and invalid input; it needs no backend/runtime. Other commands and duplicate flags reject `--check`. This is a Rust CLI addition; recursive discovery and C CLI parity remain separate. The unavailable `/decision` skill is replaced by this established format.
