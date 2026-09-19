+++
schema_version = 1
id = "01M2XHZ8Z973X121XSQJVQECNB"
title = "Runtime memory API: `alloc/dup/drop` abstraction with Boehm bridge"
date = "2026-02-06"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Accepted
* **Decision**: I will standardize runtime memory ownership operations on `fern_alloc`, `fern_dup`, and `fern_drop`, with `fern_free` retained as a compatibility alias to `fern_drop`.
* **Context**: Milestone 7.7 Step A requires an explicit memory abstraction surface before Perceus object headers and codegen dup/drop insertion land. The runtime already had `fern_alloc`/`fern_free`, but no ownership-duplication primitive or stable drop semantics that future RC backends can target.
* **Consequences**: Boehm-backed runtime now exposes stable `dup/drop` symbols with no-op ownership semantics under GC while preserving API shape for future RC backends. C-ABI regression coverage for this contract is added in `tests/test_runtime_surface.c`.
