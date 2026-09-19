+++
schema_version = 1
id = "01M2XHZ8ACD6JSRC94REEPNN8V"
title = "Prove bounded recursive Result builder contracts"
date = "2026-09-12"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted; bounded contracts and platform gates verified
* **Decision**: I will prove finite fresh-output and complete-input-retention contracts for recursive Result builders, preserving exact aliases first and never granting provisional handling credit.
* **Context**: Alias-only recursive summaries reject useful finite recursive values. Assuming recursive calls consume their inputs would instead permit silent error loss. Inductive retention summaries allow construction without assuming handling.
* **Consequences**: Successful exits must retain all promised duties; fresh obligations remain separate. List and nominal accumulators, optional payloads, generics and mutual recursion share a bounded proof engine. Widened groups rebuild dependent summaries within the original 400,000-step budget. Opaque nominal cuts do not prove descent or nonempty collections. Unknown-key Map overwrite, consuming/replacing accumulators and arbitrary higher-order equations remain conservatively rejected. See docs/RESULT_HANDLING.md.
