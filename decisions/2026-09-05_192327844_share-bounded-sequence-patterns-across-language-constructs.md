+++
schema_version = 1
id = "01M2XHZ8Q4R812MMNHKN22W35C"
title = "Share bounded sequence patterns across language constructs"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will support exact list patterns and list/tuple suffix patterns ending in `..name` or `.._` through the common checked pattern engine. Potentially failing destructuring requires match or let-else; ordinary let, for and with success bindings retain their existing irrefutability requirement.
* **Context**: DESIGN specifies list and tuple rest patterns, including function clauses, but its plain-list destructuring examples do not explain length mismatch. Silently reading beyond a list or introducing an unchecked failure would violate the existing binding contract.
* **Consequences**: List lengths and nested tags are checked before projections. Named tails are materialized only after the whole structural pattern succeeds and before any guard that uses them; ignored tails allocate nothing. List tails initially copy a bounded suffix and preserve immutable aliases. Tuple tails retain tuple identity, including singleton tuples, while an empty suffix is Unit. Match coverage models empty/nonempty lists and remains bounded under sequence expansion. Rest must appear last and can only bind or discard; Result-bearing values cannot be silently discarded by prefix or suffix patterns. Refutable plain-list examples require an else branch or match until a stronger static length proof exists. The unavailable `/decision` skill is replaced by the established decision format.
