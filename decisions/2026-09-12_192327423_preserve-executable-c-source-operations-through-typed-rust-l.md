+++
schema_version = 1
id = "01M2XHZ89Z3DK78CQY3XG2RZFT"
title = "Preserve executable C source operations through typed Rust lowering"
date = "2026-09-12"
status = "accepted"
tags = ["rust"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for migration compatibility
* **Decision**: I will preserve the shipping service aliases, bracket list indexing and infix membership through existing runtime identities and checked typed operations. Membership retains needle-before-list evaluation order.
* **Context**: A complete 212-name registration inventory and C lowering audit found these concrete executable compatibility gaps. Parser-only constructs and known C miscompilations are not valid native-output references.
* **Consequences**: Indexing shares List.get fault, full-width transport and Result-obligation rules; formatting canonicalizes it to List.get. Membership keeps IEEE comparisons and scalar Contains requirements. Native tests cover both backends, exact C reference outputs where valid, source order and deferred fault cleanup. Rust retains documented JSON, Option and error-handling corrections. Full future-language features remain distinct from compiler migration acceptance; see docs/LANGUAGE_PARITY.md.
