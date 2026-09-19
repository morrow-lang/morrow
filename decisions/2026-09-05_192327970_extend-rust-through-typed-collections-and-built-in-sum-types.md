+++
schema_version = 1
id = "01M2XHZ8V2ZQC09CYEJGVQZRB6"
title = "Extend Rust through typed collections and built-in sum types"
date = "2026-09-05"
status = "accepted"
tags = ["rust"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for the incremental Rust frontend
* **Decision**: I will extend the Rust pipeline with recursive concrete List/Option/Result types, checked constructor inference, immutable list operations, exhaustive pattern matching, and postfix Result propagation before adding user-defined generic types. Resolved IR must contain no inference variables.
* **Context**: The user authorized continuing the measured Rust migration. Compound values test the type/ABI boundary more meaningfully than adding isolated scalar syntax. Existing packed Option runtime functions truncate payloads to 32 bits and cannot safely carry Strings or full Fern Int values.
* **Consequences**: Rust Option values use the existing heap-backed Result allocation/tag/payload helpers internally (Some maps to Ok; None to Err with an unused zero payload). This preserves 64-bit payloads without changing the shipping C compiler or its packed Option ABI. Calls to C APIs returning packed Options remain unsupported until explicit adapters exist. Lists and Results reuse their existing runtime representations. Match checking initially supports scalar literals, catchalls, and built-in constructor patterns with binding/wildcard payloads; unsupported nested patterns/guards receive diagnostics. General custom types, generics, and wider tooling remain subsequent milestones. The unavailable `/decision` skill is replaced by this established decision format.
