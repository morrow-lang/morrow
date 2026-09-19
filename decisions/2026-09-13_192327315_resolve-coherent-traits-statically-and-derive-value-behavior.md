+++
schema_version = 1
id = "01M2XHZ86KHVSHZRS83QTAJ8GS"
title = "Resolve coherent traits statically and derive value behavior explicitly"
date = "2026-09-13"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; checker, native, REPL, WASM and seeded semantic tests pass

## Decision

Support single-parameter traits, default methods, parent bounds, explicit `where` requirements and coherent generic implementations. Resolve concrete methods during specialization. Derive Show, Eq, Ord and Clone for supported structural values; keep floating-point and Map ordering absent rather than inventing a total order.

## Context

The language design promises reusable checked behavior beyond intrinsic operators. Abstract methods need conservative proof contracts without executable placeholder bodies. Module ownership and overlap checks keep dispatch predictable for library authors.

## Consequences

Implementations require ownership of the trait or nominal target, exact method contracts and satisfied requirements. Resolution has shared work/depth bounds. The executable contains ordinary functions, with no runtime dictionaries or trait objects. Abstract function identities remain reserved through closure lifting. Private inference, module visibility, multiple clauses, formatting and generic Result provenance participate in acceptance. See `docs/TRAITS.md`.
