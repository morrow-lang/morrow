+++
schema_version = 1
id = "01M2XHZ8DW98C73HX83NJCM1WM"
title = "Format source directories after complete input validation"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for the Rust formatter
* **Decision**: I will support an explicit source directory in `fern-rs fmt` and `fmt --check`, using the same bounded source discovery as documentation commands. Validate every source and stage every changed file with its original permissions before publishing any replacement.
* **Context**: The design specifies `fern fmt src/`, but the Rust CLI treated directories as files. Formatting files as they are discovered would leave a partially formatted project after a later syntax or staging error. The proposal was recorded before eight failing CLI tests; implementation makes those tests pass and preserves the six existing file-check regressions.
* **Consequences**: Discovery skips hidden/build/dependency directories and child symlinks, sorts source paths, and limits depth to 32, entries to 8192, files to 256 and paths to 4096 bytes. Explicit root symlinks remain supported. Input is limited to 1 MiB per file and 8 MiB per invocation; formatted output also has an 8 MiB aggregate cap. Check mode reports all dirty paths without staging or changing bytes, permissions, inode identities or modification times. Preparation failure preserves all originals; publication uses atomic per-file renames and is not a directory-wide transaction. An OS rename error may follow earlier completed replacements. No concurrent-editor transaction, implicit path selection or watch mode is promised. Twelve new boundary/CLI/staging regressions cover these rules. The unavailable `/decision` skill is replaced by this established format.
