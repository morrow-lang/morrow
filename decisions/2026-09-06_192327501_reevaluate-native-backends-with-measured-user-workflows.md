+++
schema_version = 1
id = "01M2XHZ8CDWS46QYZGXCARGQ6W"
title = "Reevaluate native backends with measured user workflows"
date = "2026-09-06"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Assessment accepted; production backend and compiler MSRV unchanged
* **Decision**: I will pursue a supported Cranelift AOT trial against the shared validated semantics and independent native oracle corpus before choosing a new default. Keep QBE as the working reference and do not select unsupported Cranelift solely to preserve Rust1.75.
* **Context**: The user requested reassessment after the Rust frontend decision. Decision1 incorrectly described QBE as emitting C: it emits target assembly. An isolated current-Cranelift experiment produces correct native scalar output and suggests that direct object emission can remove assembler overhead. It is not an end-to-end Fern performance or compatibility result. The unavailable `/decision` skill is replaced by this established format.
* **Consequences**: A production trial must settle the supported stable toolchain policy, extract shared semantic lowering and fixed-signature runtime calls, and verify all native widths/layouts/faults/defers/GC paths. Current Cranelift requires a newer compiler toolchain; a Rust frontend itself does not require a Rust backend. Debug data, Apple object-unwind support and a future browser-WASM target require separate work. The measured scalar runtime speeds are similar; no general generated-code speed claim follows. See [the sourced assessment, measurements and acceptance plan](../docs/history/BACKEND_REASSESSMENT.md).
