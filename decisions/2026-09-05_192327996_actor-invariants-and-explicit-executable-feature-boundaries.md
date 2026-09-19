+++
schema_version = 1
id = "01M2XHZ8VWTHXXTCAZGWZQF27C"
title = "Actor invariants and explicit executable-feature boundaries"
date = "2026-09-05"
status = "accepted"
tags = ["architecture", "runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Decision**: I will enforce acyclic single-owner supervision, one replacement per dead PID, zero-safe restart windows, and stopped-sibling preservation. Native build/run will reject unimplemented spawn/receive execution with actionable diagnostics while parse/check can still inspect planned syntax.
* **Context**: Runtime defects violated existing lifecycle promises; code generation previously created actor records without executing functions and evaluated receive arms without receiving messages. Those successful compilations concealed incorrect behavior.
* **Consequences**: Mailbox APIs remain executable, `send` preserves its real Result, and unsupported actor execution fails clearly. Full scheduling and descendant supervision are still required. Rejected registrations leave state unchanged; normal/shutdown children stay stopped unless explicitly restarted.
