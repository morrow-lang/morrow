+++
schema_version = 1
id = "01M2XHZ8TSYBQYRNAHQF7GC6YW"
title = "Specialize generic code and preserve nominal type layouts"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will represent user types nominally, retain generic parameters in source syntax, and specialize generic functions and layouts into concrete typed IR. Custom sum/record values use GC-allocated storage containing a full-width discriminant and full-width fields; nested patterns inspect tags before reading payloads.
* **Context**: The user authorized completing the remaining migration milestones. Generic definitions and user types must scale beyond the initial built-in List/Option/Result cases while preserving the emitter's concrete-type boundary. The existing C runtime already exposes GC allocation.
* **Consequences**: Specialization, type expansion, and recursive matching are bounded and produce diagnostics when limits are exceeded. Runtime representations remain independent of source names; records use one constructor with named fields. Subsequent module loading must qualify declarations before type resolution. C remains available until executable-feature and tooling parity is verified. The unavailable `/decision` skill is replaced by this established decision format.
