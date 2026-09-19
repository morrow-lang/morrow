+++
schema_version = 1
id = "01M2XHZ8TH12EBTPJJVKYRH2XX"
title = "Preserve IEEE Float values across native and payload boundaries"
date = "2026-09-05"
status = "accepted"
tags = ["architecture"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Decision**: I will lower Float as QBE double values, bitcast their 64-bit representation at generic collection/sum/record boundaries, and keep integer and floating arithmetic explicitly separate.
* **Context**: Fern specifies IEEE 754 doubles. Treating generic payload bits as numerical integers would corrupt values; raw-bit equality would mishandle signed zero and NaN.
* **Consequences**: Decimal/exponent literals, arithmetic/comparisons and printing use double semantics. Numeric literals must remain finite; runtime operations may produce IEEE infinities/NaNs. Printing uses system printf with 17 significant digits. Float List.contains remains rejected until value-aware lowering exists. Integer-to-Float coercion is not implicit.
