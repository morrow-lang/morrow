+++
schema_version = 1
id = "01M2XHZ8PWA3AKDQW5SBQMGWJ0"
title = "Preserve indentation for block expressions inside delimiters"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will preserve bounded indentation frames for multiline expression suites inside calls, lists and tuples, including inline separating commas and closing delimiters.
* **Context**: Suppressing all layout within parentheses prevented valid composition such as `println(match value: ...)`. Block callbacks already needed a limited version of the same mechanism. Users should not need a temporary variable merely to pass an expression to a function.
* **Consequences**: Match, if, for, with and callback suites restore significant layout at their owning delimiter depth. Ordinary nested delimiters still suspend layout. Frames close only their owned indentation before separators/closers; malformed or excessive nesting reports a source diagnostic. Comment and multiline-string contents do not become layout instructions. Formatting must retain equivalent checked IR and remain idempotent. The unavailable `/decision` skill is replaced by the established decision format.
