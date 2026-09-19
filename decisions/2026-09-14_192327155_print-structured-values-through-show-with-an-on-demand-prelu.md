+++
schema_version = 1
id = "01M2XHZ81KCTK6SK4AKQWQX9NM"
title = "Print structured values through Show with an on-demand prelude"
date = "2026-09-14"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; checker, REPL and native output tests pass
* **Decision**: `print`/`println` keep the direct path for `Int`, `Float`, `Bool` and `String`. For lists, maps, options, results, tuples, nominal types and unions the checker rewrites the argument to a call of the `Show` trait method, using exactly the path a hand-written `show(value)` takes. When the trait prelude is not active, the checker fails with a marker diagnostic and the pipeline reruns once with the prelude forced, unless the program itself declares a prelude name. Pair tuple instances are always derived because pairs arise from `List.zip`, `List.enumerate` and runtime results without tuple syntax.
* **Context**: `println([1, 2])` was rejected with a scalar-only message although a derived `Show` already existed for every structural type. Activating the prelude for every program would add parsing and checking work and would collide with user declarations of `Ordering` or `show`. Types are unknown before checking, so activation must be demand-driven.
* **Consequences**: Programs printing only scalars are unchanged and pay nothing. Programs printing structured values check twice at most, and only when they use no other trait feature. Generic parameters initially kept the existing print capability, so `fn f(x: a): println(x)` required scalar instantiations; Decision 153 routes them through `Show` as well. `Unit` and function values remain rejected. A missing `Show` reports the rendered type with a derive hint. `Show(String)` was the raw text at this point; Decision 153 changed it to a quoted literal.
