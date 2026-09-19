+++
schema_version = 1
id = "01M2XHZ8KEGY3EMXZXVA20N10A"
title = "Expose typed Rust JSON values through explicit native adapters"
date = "2026-09-05"
status = "accepted"
tags = ["rust"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will migrate the Rust frontend to opaque json.Value/json.Error types, immutable builders and lossless ordered collection access using explicit full-width native ABI adapters. The Json compatibility spelling resolves to the same identities.
* **Context**: The native JSON parser is verified, but the existing source API still copies Strings. Native JSON member records and Fern tagged tuples have different layouts, and Float arguments require a floating-point ABI rather than an integer bit argument. The C frontend has separate qualified-type and container limitations that must not be hidden by changing registry declarations alone.
* **Consequences**: Rust parse returns Result(Value, Error) and stringify accepts Value. Explicit conversions preserve exact numbers, Unicode and NUL errors. Members return JSON String keys as Values, and object builders convert a once-evaluated Map into checked parallel native lists. Adapter allocations and expanded shared subtrees are bounded before publication, including depth/node/output growth. Opaque values cannot be fabricated, inspected as records or implicitly compared/printed. The C source contract and old native symbols remain explicitly legacy until their own migration; Rust REPL evaluation stays explicitly unavailable for these operations until the following parity checkpoint. Ten native output cases and twelve semantic rejections define this vertical migration. The unavailable `/decision` skill is replaced by the established decision format.
