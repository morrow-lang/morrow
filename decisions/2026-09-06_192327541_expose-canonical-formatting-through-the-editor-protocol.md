+++
schema_version = 1
id = "01M2XHZ8DNPBTNYTR1ACEP3CGZ"
title = "Expose canonical formatting through the editor protocol"
date = "2026-09-06"
status = "accepted"
tags = ["architecture"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for full-document Rust LSP formatting
* **Decision**: I will advertise `documentFormattingProvider` and handle `textDocument/formatting` using the accepted current open buffer and the existing syntax/comment-preserving formatter. Return no edits for canonical source or one whole-document UTF-16 replacement for changed source.
* **Context**: The CLI formatter was available but editors could not request it. The [LSP formatting contract](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_formatting) separates proposed edits from client application. Seven failing protocol tests preceded implementation; an additional executable test verifies the actual CLI transport without native backend dependencies.
* **Consequences**: Formatting does not read disk imports, change accepted buffers/versions, publish diagnostics or write files. Syntax errors return RequestFailed (-32803); malformed options and unopened documents return InvalidParams (-32602), with existing lifecycle errors preserved. Validate standard option types, a positive protocol uinteger tabSize and scalar extension values, while retaining Fern's canonical four-space style regardless of whitespace preferences. Existing source/frame bounds apply. The client applies edits to the corresponding snapshot. Range/on-type formatting, rename and code actions remain separate. The unavailable `/decision` skill is replaced by this established format.
