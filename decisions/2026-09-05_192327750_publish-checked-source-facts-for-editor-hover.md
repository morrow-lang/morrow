+++
schema_version = 1
id = "01M2XHZ8M6QP0NJ0E0K5TTTJGW"
title = "Publish checked source facts for editor hover"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for the authorized T2a Rust tooling milestone
* **Decision**: I will expose bounded source-facing type facts from finalized function validation, retaining original declaration and binding origins. Editor hover and valid-source member details will use these facts only after the complete ordinary checker succeeds on the current module overlay graph.
* **Context**: Source navigation already tracks scopes, aliases and exact UTF-16 locations. Final backend IR contains specialized copies and generated dispatch/closure names, while inference probes contain provisional variables; neither can safely define user-facing generic hover identities. Shared source presentation now validates types and patterns and explicitly renames inferred quantified identities without conflating them with declared variables.
* **Consequences**: Generic declaration schemes and instantiated occurrence types remain distinct; intrinsic requirements are reported in checker-owned language. Clauses and captured/shadowed locals preserve source anchors. Metadata allocation and output are bounded and optional, and ordinary compilation retains its existing API and behavior. Invalid current source yields no stale type facts. T2a covers hover and typed details on valid source; the separate T2b recovery milestone will handle incomplete receiver/member syntax. The unavailable `/decision` skill is replaced by the established decision format.
