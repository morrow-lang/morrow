+++
schema_version = 1
id = "01M2XHZ8MP12YY6V9YPFV3VEDQ"
title = "Replace the JSON string-copy baseline with opaque JSON values"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for staged Rust migration completion

## Decision

I will implement immutable opaque json.Value and json.Error types with a validating parser, explicit conversions and bounded serialization. New native symbols preserve the old String-copy ABI until each frontend's source API is migrated and verified.

## Context

Existing json.parse and json.stringify copy strings without validating JSON. A dynamic value model must preserve exact number text and valid JSON strings containing escaped NUL, even though Fern String cannot currently represent NUL. Typed derive/decode codecs need this foundation first.

## Consequences

The public migration will use Result(json.Value, json.Error) and Result(String, json.Error); String-as-Value calls become type errors. JSON numbers retain their lexemes, objects preserve insertion order and reject duplicate decoded keys, invalid Unicode/unpaired surrogates fail, and one leading UTF-8 BOM is accepted. Parsing is bounded to 1 MiB input, depth 128, 100,000 values and 32 MiB logical allocation; encoding is bounded to 16 MiB with charged traversal. JSON-to-Fern String conversion rejects embedded NUL without truncation. Native runtime, frontend/ABI and REPL/builders land as separate verified checkpoints; lowercase json remains canonical and existing Json compatibility spelling is preserved when migrated. The format profile follows RFC 8259 with explicit stricter duplicate/Unicode choices. The unavailable `/decision` skill is replaced by the established decision format.
