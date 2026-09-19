+++
schema_version = 1
id = "01M2XHZ86259HXWZYXENGY68DV"
title = "Implement immutable Sets over the existing traced collection representation"
date = "2026-09-13"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted; checker, REPL, native simulations and precise-GC tests pass
* **Decision**: Give `Set(a)` a sealed nominal identity with hidden `Map(a, Unit)` storage. Lower thirteen Set operations and membership into checked collection operations, preserving deterministic insertion order and existing key equality.
* **Context**: Sets need distinct source types without a second collector representation or a backend-specific implementation. Exposing the underlying Map would break the abstraction and complicate generic APIs.
* **Consequences**: Set/Map interchange and user construction of hidden storage reject. Current key domains are Int, Bool, String and supported scalar newtypes. Model-based tests check membership, ordering, persistence and full-width keys. Forced collection exposed and fixed the Map.put output-list root across nested pair allocation. A separate Map.keys Result-shape fix gives its fresh key list accurate provenance without acknowledging Results in map values. See `docs/SETS.md`.
