+++
schema_version = 1
id = "01M2XHZ9662NTPK8WE6SAVDN4X"
title = "Labeled arguments for clarity"
date = "2026-01-27"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Adopted
* **Decision**: I will require labeled arguments for same-type parameters and all Boolean parameters.
* **Context**: Function calls like `connect("localhost", 8080, 5000, true, false)` are impossible to understand without checking the definition. Which number is the port? What do those booleans mean? Labeled arguments solve this: `connect(host: "localhost", port: 8080, timeout: 5000, retry: true, async: false)` is immediately clear. The compiler enforces labels when (1) multiple parameters have the same type, or (2) any parameter is a Boolean, preventing ambiguous calls.
* **Consequences**: Function calls are more verbose but dramatically more readable. The parser must support labeled argument syntax. The type checker must enforce label requirements.
