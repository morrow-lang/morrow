+++
schema_version = 1
id = "01M2XHZ90QCM4XXKNR5MQNR4S8"
title = "Four Pillars philosophy - joy, one way, no surprises, jetpack"
date = "2026-01-29"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Adopted
* **Decision**: I will design Fern around four core pillars: (1) Spark Joy - FP should feel delightful, (2) One Obvious Way - avoid "many ways to do it" confusion, (3) No Surprises - prevent bugs that waste debugging time, (4) Jetpack Included - batteries included like Bun/Elixir.
* **Context**: Needed to articulate what makes Fern distinctive beyond just "functional + Python syntax". The four pillars capture the user experience goals: joy for FP practitioners, clarity for teams, safety by default, and productivity through included batteries. This philosophy influences every design decision - from syntax choices to stdlib scope to error messages.
* **Consequences**: README and DESIGN.md updated with philosophy. All future features evaluated against these pillars. "No surprises" particularly important - we actively prevent null, unhandled errors, race conditions, silent failures. "One obvious way" means we document idioms clearly and avoid redundant features. "Jetpack" means stdlib includes actors, DB, HTTP, TUI, CLI tools - not just basics.
