+++
schema_version = 1
id = "01M2XHZ8HSWZF2NWH8CQBSYVKT"
title = "Separate module type and value namespaces"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will keep type and value namespaces independent through declarations, visibility, imports, reexports and source navigation. Each declaration retains its own public provenance; spelling-only export metadata does not authorize visibility.
* **Context**: An alias or nominal owner may share a name with a function or unrelated constructor. A combined symbol table either rejects these valid declarations or exports a private sibling accidentally. Constructors belong to the value namespace and inherit visibility only from their nominal owner; aliases introduce no constructors.
* **Consequences**: Annotations resolve visible types; calls, pipes, function values and patterns resolve visible values after checking lexical receivers. Same-namespace collisions remain errors. Selected imports may bring both public identities into scope; editor definition returns exact type-then-value locations, deduplicating identical record anchors. Parser-confirmed import delimiters determine selector roles. Formatting and documentation preserve independent visibility and declaration ownership. Combined editor symbol accounting retains the 100,000-entry/8 MiB metadata caps and existing bounded graph/snapshot profiles. No runtime or executable IR representation changes are required. The unavailable `/decision` skill is replaced by the established decision format.
