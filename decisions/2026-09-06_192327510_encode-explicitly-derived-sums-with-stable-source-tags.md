+++
schema_version = 1
id = "01M2XHZ8CPT2AC64ZEVTA0HV6M"
title = "Encode explicitly derived sums with stable source tags"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Accepted for tagged sums and conservatively disjoint union codecs (J6a–J6b)

## Decision

I will encode derived sum values as a strict object with `tag` and `fields`, using original unqualified constructor spelling and source-order payloads. Validate the complete envelope before converting payloads; unknown variants use code13 at `/tag`.

## Context

The proposal and failing native/REPL/public-plan tests preceded implementation. Constructor ordinals and runtime storage are not stable wire identities. Finite recursive schemas need an OR of constructor products, checking all stored components, including inactive variants. The unavailable `/decision` skill is replaced by this established format.

## Consequences

Independent variant-name layout metadata validates source identity. The four-word native descriptor uses a narrowly typed children/variants pointer union; kind12 points to three-word variant descriptors. This external ABI exception does not fabricate language tuple layouts. Every envelope node and payload spends the existing shared native/REPL limits. Plan validation charges metadata and finite-value proof before publication, including inactive entries. Constructor rename/payload reorder changes the wire format. Result/function payloads remain unsupported; phantom arguments remain irrelevant. J6b uses canonical union plans and precharged shallow kind/key/length/tag profiles, validated independently even in inactive plans. Select exactly one member before decoding; no trial conversion or candidate allocation. Code14 reports no unique member at the current path. Known overlaps reject even alongside symbolic types; whole-union and exact String-key requirements survive until independently inferred specialization. Non-union signature evidence resolves before exact union equations, without a witness or directional-subtyping bypass. Inline union-bearing decoder targets reuse the existing type grammar and canonical codec identity, including nested containers and generic types. Lazy linear token metadata bounds recognition without repeatedly scanning ordinary nested expressions; ordinary calls, closures, formatting and source identities retain their existing behavior. Deeper value discrimination and general/custom traits remain open (J6c–J7). See [the codec contract](../docs/JSON_TYPED_CODECS.md).
