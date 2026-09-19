+++
schema_version = 1
id = "01M2XHZ8ZJJ43FEM51TV48THV9"
title = "Stabilize Gate C placeholder runtime behavior with error-return semantics"
date = "2026-02-06"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will make Gate C placeholder runtime functions return deterministic `Err(FERN_ERR_IO)` results for unsupported or invalid placeholder paths, instead of aborting via assertions on user-provided empty inputs.

## Context

Task 1 stabilization required runtime behavior to be documented and regression-tested, not only checker/codegen symbol mapping. Existing placeholder functions (`json.parse`, `json.stringify`, `http.get`, `sql.open`) aborted on empty-string inputs in debug builds, which broke API predictability and testability.

## Consequences

Runtime placeholder contracts are now explicit in `docs/COMPATIBILITY_POLICY.md` and covered by `tests/test_runtime_surface.c`. Empty-input behavior is stable (no abort), and future runtime implementations can evolve behind these contracts with compatibility tracking.
