+++
schema_version = 1
id = "01M2XHZ8S8P2XWCM09T3JDQCHW"
title = "Preserve immutable map and record-update semantics"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for Rust migration completion

## Decision

I will implement maps with Int, Bool or String keys and arbitrary concrete values, using semantic key equality and immutable GC-managed pair storage. Entries iterate in insertion order; replacing a duplicate key keeps its position and the last value wins. Record updates evaluate their base and field expressions once in source order before constructing a fresh record.

## Context

Fern specifies Map literals and new/get/put/delete, but does not define key equality or ordering. The C runtime has no Map ABI, and its record-update emitter currently returns the unchanged base. Compiler-owned typed lowering gives native and interactive execution one explicit contract without inferring types from transport widths.

## Consequences

Map lookup and immutable updates initially take linear time; hashing is a later optimization behind the same semantics. Float and compound keys are rejected until an equality/hash contract is specified; values retain full-width Float, pointer, closure and Result representations. Deleting and reinserting a key appends it. Native tests use semantic expected output, including aliases, duplicate effects and record updates, rather than inheriting C's incomplete behavior. The source API also provides len/is_empty/contains/keys/values for inspection. Unknown or duplicate update fields are diagnostics. The unavailable `/decision` skill is replaced by the established decision format.
