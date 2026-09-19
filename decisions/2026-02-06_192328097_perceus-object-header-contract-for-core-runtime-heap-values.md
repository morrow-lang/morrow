+++
schema_version = 1
id = "01M2XHZ8Z1496XT7551SC84WDZ"
title = "Perceus object header contract for core runtime heap values"
date = "2026-02-06"
status = "accepted"
tags = ["architecture", "runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Accepted
* **Decision**: I will add a stable Perceus-style object header API (`fern_rc_alloc`, `fern_rc_dup`, `fern_rc_drop`, and metadata accessors) and tag core runtime heap allocations (`Result`, `List`, `StringList`) with explicit RC type tags.
* **Context**: Milestone 7.7 Step B requires concrete runtime object metadata and refcount operations so later codegen work can insert dup/drop in a verifiable way. Step A only established abstraction entry points (`alloc/dup/drop`) without object header semantics or typed heap metadata.
* **Consequences**: Runtime now exposes header-level refcount/type/flag queries and updates, and core heap constructors use RC-tagged allocations while memory reclamation remains Boehm-driven for now. Compatibility and C-ABI regression coverage are extended in `tests/test_runtime_surface.c` (`test_runtime_rc_header_and_core_type_ops`).
