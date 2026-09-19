+++
schema_version = 1
id = "01M2XHZ8VMAQGDE4N1ZXH3XAM6"
title = "Verify native diagnostics before replacing the reference checker"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will require exact diagnostic multisets and exit codes on pinned failing fixtures and repository source before claiming native checker diagnostic parity; Python remains the default until the complete build/git/CLI workflow is validated.

## Context

Comparing successful exit codes alone hid missing checks and a native main function that always exited successfully.

## Consequences

CI requires diagnostic parity; full checker replacement remains an explicit open task. String constants use bounded printable runs and numeric unsafe bytes to preserve assembler-independent content and avoid truncation.
