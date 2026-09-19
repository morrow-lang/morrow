+++
schema_version = 1
id = "01M2XHZ8KYH8NAXPQKVWK62S8T"
title = "Expand transparent aliases before nominal checking with shared budgets"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will implement scalar and generic transparent type aliases as source declarations, expanding their references before nominal registry construction without adding an executable representation. Alias expansion charges the same bounded work budget used by dependency analysis and inference.
* **Context**: DESIGN distinguishes transparent aliases from distinct zero-cost newtypes and set-theoretic unions. Reusing tagged one-field records for all three would change their promised identity or runtime cost. Aliases can provide useful source vocabulary while retaining existing type equality and runtime layout.
* **Consequences**: Original alias declarations and module identities remain available to formatting, documentation and editor navigation. Expansion substitutes generics without capture, rejects arity/name/cycle errors, and checks depth/node/output budgets before allocating expanded trees. Nominal recursive records remain valid; transparent cyclic aliases do not. Aliases add no constructors and do not create a privacy boundary, while existing constructor visibility stays enforced. Newtype representation and union coercion/narrowing remain separate checkpoints. The unavailable `/decision` skill is replaced by the established decision format.
