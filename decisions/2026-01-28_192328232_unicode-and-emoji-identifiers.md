+++
schema_version = 1
id = "01M2XHZ938M17WQ65G45NG6SKE"
title = "Unicode and emoji identifiers"
date = "2026-01-28"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Accepted
* **Decision**: I will allow Unicode letters and emojis as valid variable/function names, following Unicode identifier standards (XID_Start/XID_Continue) plus emoji support.
* **Context**: The question arose whether to restrict identifiers to ASCII or allow broader Unicode. Considered: (1) ASCII-only - simple but excludes international developers and mathematical notation, (2) Unicode letters only (XID categories) - allows π, θ, non-Latin scripts but not emojis, (3) Full Unicode + emojis - maximum expressiveness. Chose option 3 because it's the developer's choice to use identifiers responsibly. Languages like Swift and Julia allow emoji identifiers. While there are practical concerns (typing difficulty, rendering inconsistency, searchability), these are tradeoffs developers can evaluate for themselves.
* **Consequences**: The lexer must recognize Unicode XID_Start/XID_Continue categories plus emoji codepoints as valid identifier characters. DESIGN.md will document identifier rules. Test cases will verify Unicode identifiers work correctly (e.g., `let π = 3.14159`, `let 🚀 = launch()`).
