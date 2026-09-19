+++
schema_version = 1
id = "01M2XHZ8J8K8PE4ENH9J3ZKCTW"
title = "Preserve distinct newtype identities without wrapper allocation"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will represent newtypes as semantic nominal identities with validated unboxed payload layouts. Explicit construction and projection lower to the same native operand, while parameter, result, container and closure boundaries use the payload's full-width ABI.
* **Context**: DESIGN promises distinct UserId/ProductId identities with zero runtime cost. A tagged one-field record would introduce allocation and change Float/native ABI behavior. Concrete layout keys distinguish valid nested wrappers from impossible unboxed cycles, while existing heap indirection supports guarded recursion.
* **Consequences**: Same-identity scalar equality and List.contains inherit Int/Float/Bool/String behavior; Map keys inherit Int/Bool/String behavior with String content comparison. Arithmetic, ordering, Print/interpolation and implicit conversion remain unavailable without explicit projection or future traits. Wrapped Result values retain handling obligations. Checked Wrap/Unwrap and newtype patterns are validated at public IR boundaries; depth/type/program-work limits apply before expansion. REPL values reuse underlying storage, while formatter/docs/editor preserve source identities. The unavailable `/decision` skill is replaced by the established decision format.
