+++
schema_version = 1
id = "01M2XHZ8R5ZQ6PSV7K2SZ3HR0J"
title = "Define numeric domains and unwind runtime faults through cleanup"
date = "2026-09-05"
status = "accepted"
tags = ["runtime", "infra"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will use wrapping Int arithmetic consistently, bounded exponentiation by squaring for nonnegative Int powers, IEEE Float power, and value-based Float list membership. Invalid integer division/remainder by zero or negative integer exponents produce controlled runtime diagnostics after deferred cleanup.
* **Context**: The existing REPL diagnoses zero division while native signed division can trap or vary by architecture. DESIGN's debug-overflow panic rule conflicts with its no-panic aspirations and the current wrapping interactive implementation. Fern's scalar operators retain scalar result types; invalid numeric domains need a defined execution failure rather than an arbitrary value or a hardware-dependent crash.
* **Consequences**: Generated functions receive an explicit fault context after their environment argument; closures receive the current caller's context and never capture a stack context. Fault checks dominate uses of function/callback results. Faulting paths run ordinary function cleanup, and the first fault wins even if cleanup also fails. Each cleanup callback runs with a cleared context, so remaining callbacks still execute. Main reports one diagnostic and exits 1. This introduces no mutable process-global state and changes neither source Function/Result types nor the C runtime ABI. Int::MIN divided by -1 wraps to MIN, with remainder 0; 0**0 is 1. Power is right-associative and retains existing unary precedence. Elixir-style bitwise operators &&&/|||/^^^/~~~/<<</>>> keep record-update and pipe delimiters distinct; shifts normalize counts modulo 64 and right shifts preserve the sign. General recoverable checked-arithmetic APIs remain a separate library requirement. The unavailable `/decision` skill is replaced by the established decision format.
