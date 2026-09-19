+++
schema_version = 1
id = "01M2XHZ8E2VB9AXK8C5C17GXKT"
title = "Derive explicit typed JSON codecs with bounded shared execution"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for concrete acyclic codecs (J4)
* **Decision**: I will provide `json.encode(value)` and `json.decode(text, TargetType)` as uniformly fallible operations, with explicit `derive(Json)` on nominal records. Decode's target is a compile-time type reference; input pipes retain source evaluation order. Native and interactive execution share concrete validated wire plans and the existing exact JSON adapters.
* **Context**: Dynamic JSON required manual field conversion and did not establish a typed record boundary. Inferring serialization from storage would silently encode Result obligations and leave unknown-field, nullability and generic behavior unspecified. Parser/checker, public-IR, native-output, REPL and resource-boundary tests preceded implementation. The unavailable `/decision` skill is replaced by this established format.
* **Consequences**: J4 supports primitives, dynamic JSON, tuples, Lists, String-key Maps, nullable-safe Options and acyclic derived records, including concrete generic instances. Reject Result-bearing values, unsupported derivations even when unused, ambiguous nullable Options and runtime decode targets. Records reject unknown fields (code12); absent safe Option fields become None, while required fields fail with code6. Errors retain stable codes/offsets and an immutable JSON Pointer; path-growth failure reports the last entered parent. Each call shares input/output/allocation/node/depth/work limits across every phase and child. Public plans validate inactive entries and bodies under one 400,000-unit allowance; native descriptor output is capped at 16 MiB. The authored runtime include belongs to source fingerprints. Recursive schemas, newtypes, generic codec constraints, sum/union wire formats and general/custom traits remain J5–J7, not implicit behavior. C keeps its legacy JSON source and ABI contract. See [the typed codec contract](../docs/JSON_TYPED_CODECS.md).
