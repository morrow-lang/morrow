+++
schema_version = 1
id = "01M2XHZ8BA017TNFA2HRS8W9V6"
title = "Package the Rust frontend as an explicit relocatable preview"
date = "2026-09-06"
status = "accepted"
tags = ["rust", "release"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for opt-in staging and verification; shipping defaults unchanged
* **Decision**: I will stage explicit already-built Rust/native components into a new immutable directory and atomically publish one completely verified archive. Preserve exactly seven sibling files and a closed, bounded canonical manifest/archive format.
* **Context**: Users need to try the expanded Rust frontend outside its source checkout. Portable nonempty directory replacement is not atomic, and checkout fallback can conceal incomplete packages. Failing package/marker/native-relocation tests preceded implementation. The unavailable `/decision` skill is replaced by this established format.
* **Consequences**: A sibling preview marker disables implicit checkout fallback even when malformed; explicit component overrides remain supported. Bound actual reads, decompression, metadata and precharged payload copies; reject links, aliases, extra paths and malformed archives before publication. Reproducibility covers identical package inputs with the same packaging toolchain, not native compilation or publisher authentication. Native compilation still requires matching host libraries. No installation, signing, publication or default switch is implied. See [the runnable packaging guide](../docs/history/RUST_PREVIEW_PACKAGING.md).
