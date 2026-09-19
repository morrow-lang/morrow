+++
schema_version = 1
id = "01M2XHZ93JXMBFK762CV1MSC2Y"
title = "Adopting TigerBeetle-inspired FERN_STYLE"
date = "2026-01-28"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will adopt a coding style guide inspired by TigerBeetle's TIGER_STYLE, with emphasis on assertion density, function size limits, and fuzzing-friendly code.

## Context

TigerBeetle's engineering practices produce extremely reliable code through: (1) minimum 2 assertions per function, (2) 70-line function limit, (3) pair assertions for critical operations, (4) compile-time assertions, (5) explicit bounds on everything. These practices align well with AI-assisted development because they make invariants explicit, keep functions small enough for AI context windows, and enable effective fuzzing. The VOPR-style deterministic simulation testing approach will be adapted as FernFuzz for grammar-based compiler testing.

## Consequences

Created FERN_STYLE.md as the coding standard. All code must meet assertion density requirements. Functions over 70 lines must be split. Fuzzing infrastructure (FernFuzz) will be added to test lexer/parser with random programs. CI will enforce style compliance.
