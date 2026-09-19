+++
schema_version = 1
id = "01M2XHZ8HA5FGHSSH3N46M6S18"
title = "Generate the indentation-aware editor grammar from authored templates"
date = "2026-09-06"
status = "accepted"
tags = ["rust"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted; bounded native/query/WASM corpus verified
* **Decision**: I will keep explicit grammar/query templates as editable sources, render every published grammar/query deterministically, and generate parser sources and ABI14 WASM only with pinned Tree-sitter 0.26.12 and WASI SDK 29.0. The Rust compiler and accepted source corpus remain the language authority.
* **Context**: The old generator silently skipped the actual indentation grammar and could not derive aliases, newtypes or function clauses from C token names. Stale WASM copies and uncompiled query text did not establish editor correctness.
* **Consequences**: The gate checks 24 accepted sources against Rust and native/WASM trees, eight bounded recovery cases, eight incremental edits, four executable queries, and scanner malformed-state/column/stack limits under sanitizers. The external scanner's explicit lifecycle exception permits Tree-sitter `ts_calloc`/`ts_free` and defensive reset/return guards instead of assertions on untrusted serialized state. All 128 indentation levels fit the 514-byte serialized state; columns are 32-bit and capped at 1 MiB. Generation checks every parser source/header plus both identical WASM copies; build output uses the canonical basename because it affects WASM metadata. Full Rust syntax parity and Zed extension registration/packaging remain open. The unavailable `/decision` skill is replaced by this established decision format.

The union follow-on extends this verified profile to 38 accepted sources, 12 recovery cases and 13 incremental edits. Structural assertions distinguish functions returning unions from function-valued union members, and typed binders from wildcards. Module-alias fixtures use real Rust module graphs; native/WASM trees and highlight captures agree. Both generated WASM artifacts are 110,316 bytes with SHA256 `fccdfd05b2db4117680058e3d6fe2c39bd8d13c02ed24d95486cb79b218d1f0a`. Broader syntax and extension packaging remain open.

The control/collection follow-through verifies 69 accepted sources, 20 malformed
inputs and 22 incremental edits with fresh per-run native caches. A serialized
post-dedent separator and exact 1 MiB indentation boundary prevent cross-line
calls and unbounded scanner work. Three named malformed inline for/with headers
still absorb the following declaration; their exact error ranges remain tracked,
without a full-recovery claim.

The label follow-on verifies 80 accepted sources, 27 malformed inputs (24 recover
and three retain their named gaps), and 27 incremental edits. External pattern/call
labels have separate parameter captures; strict Rust checks all accepted and edited
sources. Both WASM copies are 243,311 bytes, SHA256
`69c118755c23ee56708e838fb6c1956a8214fb2d0b0c5760a715d92b1a46f88c`.
The matching Zed grammar revision must be repinned after this grammar is committed.

The recovery follow-on closes all three original inline-header gaps while preserving
their source bytes. Hidden prefix reductions and contextual declaration/lambda `fn`
tokens retain real missing-token/error nodes and all following declarations. The
full native/WASM profile now verifies 85 valid, 33 malformed and 30 incremental
cases, with scanner keyword boundaries and lookahead under the existing limits.
Both WASM copies are 277,598 bytes, SHA256
`27f01d3d422bad335369b4069fc86c7239b097180ba5f5dc710ed5aab3d12fef`.

The numeric editor follow-on verifies 93 accepted sources, all 33 existing
malformed recovery ranges and 33 incremental edits. Binary/octal/hexadecimal
prefixes, valid integer separators and exponent-only Floats retain exact numeric
token kinds/text. All published parser/WASM files are generated from the authored
template; remaining syntax and numeric semantic validation still belong to the
compiler. This extends Decision84 without changing Fern numeric semantics.
