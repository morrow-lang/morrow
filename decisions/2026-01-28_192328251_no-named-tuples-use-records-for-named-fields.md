+++
schema_version = 1
id = "01M2XHZ93VGTXK8D1M0T4CA1X5"
title = "No named tuples (use records for named fields)"
date = "2026-01-28"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Adopted
* **Decision**: I will not support named tuple syntax `(x: 10, y: 20)`. Use positional tuples `(10, 20)` or declared records for named fields.
* **Context**: Named tuples create confusion because they look like records but aren't declared types. Users wouldn't know when to choose named tuples vs records. Keeping a clear distinction simplifies the mental model: tuples are positional and anonymous `(a, b, c)`, records are declared with `type` and have named fields. If you need named fields, declare a type.
* **Consequences**: Tuple syntax is positional only: `(10, 20)`. Named fields require a `type` declaration. Simpler grammar, clearer semantics, no ambiguity about tuple vs record.
