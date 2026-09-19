+++
schema_version = 1
id = "01M2XHZ8RETYX3HK4793CDXEYM"
title = "Preserve literal contents and document Unicode identifier spelling"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will parse base-prefixed integers with explicit digit/separator/range validation, preserve the exact contents of triple-quoted strings, and accept bounded nested block comments. Documentation attributes retain their declaration association. Non-ASCII non-whitespace identifier characters retain the C frontend's broad spelling compatibility, with exact UTF-8 identity and no normalization.
* **Context**: Fern specifies multiline strings, block comments, documentation attributes and full-width numeric values, while legacy lexical acceptance includes incomplete or malformed cases. Reusing ordinary strings' escapes/interpolation and retaining newline/indent bytes avoids implicit transformations. Bitwise token choices must coexist with record updates and pipes.
* **Consequences**: Unterminated comments/strings and invalid digits, separators or integer magnitudes are diagnostics. Case-insensitive 0x/0b/0o prefixes select bases; a leading minus permits the exact Int minimum. String contents do not undergo automatic dedenting. ASCII identifiers begin with a letter or underscore and continue with letters/digits/underscores; non-ASCII spelling is preserved exactly. Formatting must preserve parsed semantics, literal values, comments and documentation metadata. The unavailable `/decision` skill is replaced by the established decision format.
