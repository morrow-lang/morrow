+++
schema_version = 1
id = "01M2XHZ8HH5681YWZKP1EXS8ZQ"
title = "Directional finite unions and typed narrowing"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for the bounded first checkpoint
* **Decision**: I will implement canonical finite ordinary-type unions with directional member/subset conversions, typed binding/wildcard narrowing and full-width tagged GC envelopes. Exact unification remains symmetric; existing containers and function types remain invariant. Generic substitutions normalize before specialization and do not guess ambiguous membership.
* **Context**: The union examples in DESIGN.md specify arguments and typed narrowing without defining principal union inference, constructor refinements, variance or lifted capabilities.
* **Consequences**: Declared union contexts permit mixed branches and fresh literals; inferred heterogeneous joins remain errors. Narrow before operators, printing or Map-key use. Result-bearing alternatives retain handling and capture obligations. Normalization, assignment and coverage are bounded, including inactive public IR metadata. Constructor refinements, variance, implicit joins and lifted capabilities remain successors. See [the union contract](../docs/UNIONS.md) for representation, limits and executable examples. The unavailable `/decision` skill is replaced by this established decision format.
