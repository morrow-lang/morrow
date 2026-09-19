+++
schema_version = 1
id = "01M2XHZ8JZJXGA6J6AAG1AKG10"
title = "Evaluate immutable JSON in the REPL with explicit aggregate limits"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will implement the dynamic JSON API in the safe, std-only Rust evaluator using immutable shared nodes and the same native format/error/resource profile. No native FFI, subprocess evaluation or lossy serialization intermediary is used.
* **Context**: Native opaque JSON values now have a tested contract. Interactive programs must retain exact number text, Unicode including escaped NUL, insertion order, ordinary Result errors and immutable shared children across session entries. A per-call parser cap alone cannot bound repeated large operations in one entry.
* **Consequences**: Decimal-to-Float conversion uses Rust 1.75's nearest/ties-even parser after strict JSON validation, and Float construction reproduces the native 17-significant-digit spelling. Logical native allocation/work charges preserve per-operation limits; normal interactive evaluation additionally has separate 64 MiB aggregate allocation and work ceilings, with independent 8 MiB cleanup reserves. Charges occur before work/allocation, including failed attempts; aggregate allocation charges include larger semantic Rust node/collection representations while native logical per-operation counters stay unchanged. Retained storage uses an iterative unique-node walk under the existing 16 MiB/200,000 session ceilings; cached expanded sizes bound serialization but never replace physical sharing accounting. JSON domain errors remain ordinary Results; aggregate faults retain existing first-failure, cleanup and atomic binding behavior. C source migration and typed codecs remain separate. The unavailable `/decision` skill is replaced by the established decision format.
