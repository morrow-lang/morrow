+++
schema_version = 1
id = "01M2XHZ856GJY1X3WEYJ885F1B"
title = "Preserve logical call and loop state in typed actor continuations"
date = "2026-09-13"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; native scheduling, ABI and deterministic polling tests pass
* **Decision**: Normalize strict operands once in source order and compile actor-reachable direct helper calls and collection loops into separate continuation functions. Typed return frames support non-tail and mutual recursion without retaining native call stacks. Keep ordinary synchronous function entry points for CLI and non-actor calls.
* **Context**: Unit tail calls alone left recursive value-producing helpers and collection loops able to monopolize a scheduler callback. Calling a nested scheduler from an ordinary native function would retain unbounded native stacks and break ownership.
* **Consequences**: List, Map and Range loops retain immutable state, lexical exits and receive behavior; inclusive maximum endpoints do not overflow. Managed return frames consume explicit resource budgets, while eligible tail calls reuse continuations. Independent tests verify sibling progress, strict operand order, full-width tuple results, Unicode and collection under seeded polling. Blocking foreign/runtime calls still require an asynchronous service adapter. Suspension eligibility and cleanup evolution are documented in `docs/ACTOR_CONTINUATIONS.md`.
