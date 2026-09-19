+++
schema_version = 1
id = "01M2XHZ8NNFSP1VE2W9VSV6YE0"
title = "Validate generic bodies with rigid equality and capability requirements"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will validate every generic body before specialization using rigid declared type variables and explicit internal requirements for overloaded operations. Type equality will no longer accept an arbitrary concrete type merely because one side is generic.
* **Context**: The current template probe can accept an unused `fn bad(x: a) -> a: 1`, while concrete specialization rejects some later uses. Existing generic arithmetic and scalar interpolation are useful and must retain their actual numeric/display restrictions rather than be checked with an arbitrary Int instance.
* **Consequences**: Capability requirements preserve the concrete domains of arithmetic, addition, ordering, equality, printing, collection membership and map keys. Calls and function values instantiate and propagate those requirements with their types, under bounded work. Concrete impossible requirements and incompatible universal returns are errors even when unused. Conditional Result discard obligations remain distinct from type equality. Concrete specialization continues to validate the backend boundary. Public where/trait syntax and whole private-signature generalization build on this internal scheme representation separately. The unavailable `/decision` skill is replaced by the established decision format.
