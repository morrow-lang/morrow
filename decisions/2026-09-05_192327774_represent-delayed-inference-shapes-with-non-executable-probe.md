+++
schema_version = 1
id = "01M2XHZ8MY3TPTECD5J3VK47C2"
title = "Represent delayed inference shapes with non-executable probe nodes"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will retain delayed field, update, tuple-rest and iteration constraints in a bounded obligation table, using an explicitly tagged probe-only IR node to continue gathering later body evidence. Construction is crate-private and requires a private token; the node is never executable Fern IR.
* **Context**: Retrying a body after its first unresolved projection cannot discover an annotation or call later in that same body. Returning a fake Unit, local or field-index value would obscure this gap and could corrupt later compiler passes. A second complete type checker would duplicate the existing typing rules.
* **Consequences**: Probe nodes retain evaluated child expressions and their result type slot; delayed obligations resolve only from independent type evidence, without guessing nominal types or tuple arity. Union/assignment revisions determine progress, and retries share the whole-signature work budget. Probes are discarded before generalized source is rechecked. Finalization, public IR validation, code generation, interactive execution and editor fact publication must reject any surviving probe. All affected visitors are updated explicitly and rejection/resource/branch-order tests guard the boundary. The unavailable `/decision` skill is replaced by the established decision format.
