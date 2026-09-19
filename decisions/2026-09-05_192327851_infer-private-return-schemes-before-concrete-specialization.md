+++
schema_version = 1
id = "01M2XHZ8QBWP1QYS534RQY341M"
title = "Infer private return schemes before concrete specialization"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will infer omitted private function returns with shared, bounded type constraints before the existing concrete specialization pass. Public return signatures remain explicit and omitted main remains Unit.
* **Context**: DESIGN permits internal inference while requiring annotated APIs. Checking definitions independently loses forward and recursive return constraints; specializing a generic probe as Int would reject valid Float uses or silently change a scheme.
* **Consequences**: Annotated parameters remain the boundary for this checkpoint. Return evidence from tails, early returns and propagation can establish concrete types or declared generic schemes. Only unresolved shape dependencies are retried; genuine errors remain errors. Unanchored cycles require an annotation. A shared work and inference-storage budget bounds retries across all definitions. Public provenance survives module flattening. This does not claim full parameter inference, function clauses, or complete unused generic-body checking. The unavailable `/decision` skill is replaced by the established decision format.
