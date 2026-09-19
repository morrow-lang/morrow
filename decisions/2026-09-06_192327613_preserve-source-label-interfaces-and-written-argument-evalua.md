+++
schema_version = 1
id = "01M2XHZ8FXQWY7PRAEVMQ3NN49"
title = "Preserve source label interfaces and written argument evaluation order"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted; direct source labels and mandatory enforcement implemented
* **Decision**: I will resolve direct source-call labels against original declaration interfaces, keeping external names distinct from pattern bindings and evaluating arguments once in written order. Reordered calls use typed local temporaries before parameter-order reads.
* **Context**: Decision7 requires readable calls, but reordering source expressions changes side effects and error propagation. Clause normalization, module aliases and generic specialization must not substitute synthetic names or call-site types for the source interface.
* **Consequences**: Positional arguments precede labeled arguments; duplicate, unknown, missing and multiply supplied positions are errors. Explicit external pattern names use `fn choose(enabled true: Bool)`. Structural function values, lambdas, runtime/compiler builtins and constructors retain positional interfaces and reject labels. Direct source calls require labels for exact Bool and repeated identical finalized declared scheme types; distinct generics/newtypes remain distinct. Classification follows whole-signature inference under a separate 400,000-unit work ceiling. Required pipe positions use labeled holes; inputs run first and once. Public metadata uses shared identifier/keyword rules and valid span ordering. Formatter/module/presentation metadata preserve source labels. Label-token definition/hover use current checked source interfaces and declared schemes; incomplete-member recovery retains label validation. Native/WASM grammar and highlights cover the bounded label corpus; incomplete-call name suggestions use parser-proven source positions and current lexical interfaces without claiming checked types or requiredness. Full syntax parity remains open. See [the label contract](../docs/LABELED_CALLS.md). The unavailable `/decision` skill is replaced by this established format.
