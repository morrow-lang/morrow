+++
schema_version = 1
id = "01M2XHZ91AYRKCJP0S0E8XZJQ2"
title = "Embedded QBE compiler backend"
date = "2026-01-29"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will embed QBE directly into the fern binary rather than requiring it as an external dependency.

## Context

The fern compiler was calling external `qbe` binary via system() which required users to install QBE separately. This conflicted with the "single binary" philosophy. Considered options: (1) Keep external qbe - simple but adds dependency, (2) Embed QBE source - removes dependency, single binary, (3) Use LLVM - powerful but massive dependency, (4) Write custom backend - flexible but huge effort. QBE is only ~6,650 lines of C with no dependencies, making it ideal for embedding. Modified QBE's main.c to expose `qbe_compile()` library function.

## Consequences

QBE source added to `deps/qbe/` (~16 files, 6.6K lines). Fern binary increased from ~200KB to ~540KB. Users no longer need to install qbe. The fern binary is now fully self-contained for development - only needs a C compiler (cc/clang) for assembling and linking, which is standard on all Unix systems.
