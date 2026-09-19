+++
schema_version = 1
id = "01M2XHZ8SS4MS0QZ0M4Q1QQ47A"
title = "Lift typed closures with explicit environments"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will infer function values through semantic Function types, specialize generic functions before lifting closures, and use a uniform hidden environment argument for generated functions. Closures retain a code pointer and full-width captured values in GC-managed storage. Higher-order builtins execute typed calls through this convention.
* **Context**: Fern's documented anonymous functions and functional collection operations require captures that survive their defining call. The legacy C callback ABI has neither an environment parameter nor complete Float/Option transport. The persistent REPL recompiles source definitions, so numeric function IDs alone cannot identify code across entries.
* **Consequences**: Captures and arguments evaluate once in source order; native Float payloads preserve their bits. Builtin and runtime function values receive concrete typed wrappers. Interactive closures retain their originating immutable checked program. Each lambda has its own return/error context. Capturing already-produced Result-bearing values is temporarily rejected because delayed callbacks may never execute; lifting this restriction requires ownership/effect tracking across closures, aliases and containers. Functions returning Results are not themselves unhandled Result values. C remains the default until full migration gates pass.
