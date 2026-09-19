+++
schema_version = 1
id = "01M2XHZ8KN1H7WQG9GC1KNKHHV"
title = "Preserve resolved global references as explicit AST identities"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will distinguish resolved global names, calls and pipe targets from lexical names in the module-resolved AST. Resolution checks the original source root against lexical bindings before producing explicit global forms.
* **Context**: Rewriting an import alias to a canonical module string can accidentally capture a different local with that canonical spelling. This affects compilation, dependency ordering and checked editor facts: `import model as m` followed by `let model = 3` must not turn `m.value()` into a field access on that local. Giving all dotted names global priority would instead break actual lexical shadowing.
* **Consequences**: Function values, direct calls, pipes, captures and generic dependency discovery retain their resolved identity. Source parsing and formatting preserve written syntax; module resolution owns the transition to explicit global forms. Every AST visitor handles these forms explicitly, and regression tests cover aliases, canonical-name collisions and real source-root shadowing. No magic string prefixes or span-only identity side tables are used. The unavailable `/decision` skill is replaced by the established decision format.
