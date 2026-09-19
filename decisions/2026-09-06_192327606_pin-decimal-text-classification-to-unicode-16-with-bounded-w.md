+++
schema_version = 1
id = "01M2XHZ8FPJ1H8EQ4H7FFQG8YW"
title = "Pin decimal text classification to Unicode 16 with bounded work"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for both native frontends and the REPL

## Decision

I will expose `String.is_decimal(String) -> Bool` and `str_is_decimal` as nonempty all-Nd classification using checksum-generated Unicode 16.0.0 tables. Native content above 16 MiB raises the existing String-size fault before classification; the Rust invocation path preserves deferred cleanup.

## Context

The pinned Python 3.14 bootstrap reference uses Unicode 16 decimal digits. ASCII-only matching and broader numeric predicates misclassify command arguments. Host Rust Unicode tables must not silently change Fern behavior.

## Consequences

A vendored primary UCD file and retained license generate 71 ranges covering 760 scalars offline. Empty and malformed native UTF-8 return false; NUL remains the native String terminator. Native scanning allocates nothing. REPL uses the same table, reserves one existing Machine step per 64 bytes before scanning, and retains its stricter storage and separate cleanup budgets. No numeric parsing, locale, normalization or new C defer guarantee is implied. [The classifier contract](../docs/STRING_DECIMAL.md) records provenance, limits and exhaustive tests. The unavailable `/decision` skill is replaced by this established format.
