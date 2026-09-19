+++
schema_version = 1
id = "01M2XHZ8DDADC212Z8AA177MZ0"
title = "Build finite recursive JSON plans and extend explicit derivation coherently"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted staged continuation; J5a recursive records, J5b newtypes and J5c conditional requirements
* **Decision**: I will represent recursive codecs as finite indexed graphs keyed by exact instantiated types, reserve private construction slots before visiting children, and publish only complete validated plans. Reject strict schema cycles with no finite value as a codec restriction; Lists, Maps and safe Options provide finite bases.
* **Context**: J4's acyclic plans reject ordinary trees even when empty children terminate recursion. Disabling the child-first check without graph validation would admit invalid references, skipped fields and unbounded generic expansion. An explicit graph preserves source nominal identity without recursively owning plan nodes. The proposal and failing tests preceded implementation; the unavailable `/decision` skill is replaced by this established format.
* **Consequences**: Validate every entry, typed edge and storage layout, including inactive entries; graph construction and finite-value proof share the existing work/count bounds. Every executed edge retains depth/work/path charges, including hostile cyclic native values. Native descriptor ABI is unchanged. J5b adds explicitly derived transparent newtypes, inherits payload nullability while retaining actual Option field optionality, and proves no native wrapper allocation. J5c retains conditional Json, JsonNonNull and exact JsonStringKey requirements through generic source schemes, recursive calls and function values. An unforgeable private template operation retains real input effects; all executable boundaries reject that operation. Concrete specialization checks the exact target again, with no default witness. Requirements follow actual stored fields, not phantom metadata; predicate and template passes retain separate shared 400,000-work bounds. Sum/union wire formats, general/custom traits and Result serialization remain separate decisions/work.
