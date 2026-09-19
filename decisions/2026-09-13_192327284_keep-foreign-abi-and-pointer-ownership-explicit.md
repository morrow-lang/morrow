+++
schema_version = 1
id = "01M2XHZ85ME6W4VTA8J48635B3"
title = "Keep foreign ABI and pointer ownership explicit"
date = "2026-09-13"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; source, native ABI, conversion simulation and forced-GC tests pass

## Decision

Support `foreign "C" fn ... -> ... as "symbol" from "logical_library"` with exact physical ABI metadata, checked compiler-owned narrow scalar types and sealed `Ptr(a)` handles. String borrows retain their owners; returned pointers conservatively retain owners from pointer arguments, including interior returns. Import UTF-8 through a bounded fallible copy.

## Context

The old raw mutable-pointer sketch did not fit Fern's immutable value and actor-isolation model. Rust adapters and mature third-party libraries remain useful, but an unrestricted Int cannot safely stand for both a C integer and a pointer.

## Consequences

Foreign declarations are a trusted native boundary. The compiler checks signatures and literal linker names; it cannot prove a foreign library's ABI declaration, address validity, freeing or retention contract. Source code cannot forge addresses, dereference pointers, mutate hidden fields, or send/serialize Ptr values. REPL, comptime and browser execution reject foreign effects. Narrow constructors preserve Result obligations; foreign-returned unsigned 64-bit values retain all bits. Independent tests exercise exact widths, 36 mixed register/stack arguments, 1,024 seeded transports, 4,096 floating bit patterns, real source/library linking and precise-GC owner lifetimes. See `docs/FFI.md`.
