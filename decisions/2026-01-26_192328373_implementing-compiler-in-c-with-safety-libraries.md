+++
schema_version = 1
id = "01M2XHZ97NNSQZ3Z7KKYVZR71B"
title = "Implementing compiler in C with safety libraries"
date = "2026-01-26"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will implement the Fern compiler in C11 with safety libraries (arena allocator, SDS strings, stb_ds collections, Result macros) instead of Rust, Zig, or C++.

## Context

Considered several implementation languages: (1) Rust - safe but complex, steep learning curve, slower compile times, (2) Zig - interesting but immature, fewer AI training examples, (3) C++ - too complex, many ways to do things wrong, (4) C with safety libraries - simple, well-understood by AI, fast compilation, full control. Using arena allocation eliminates use-after-free and memory leaks. Using SDS eliminates buffer overflows. Using Result macros eliminates unchecked errors. C is also extremely well-represented in AI training data, making AI-assisted development highly effective.

## Consequences

Must use arena allocator exclusively (no malloc/free). Must use SDS for all strings. Must use stb_ds for collections. Must use Result types for fallible operations. Compiler warnings must be treated as errors.
