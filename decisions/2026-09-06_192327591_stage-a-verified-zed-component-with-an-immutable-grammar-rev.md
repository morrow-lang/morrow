+++
schema_version = 1
id = "01M2XHZ8F7RSHVSQRRARJJPXQN"
title = "Stage a verified Zed component with an immutable grammar revision"
date = "2026-09-06"
status = "accepted"
tags = ["release", "performance"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for local packaging and isolated actual-editor smoke tests
* **Decision**: I will register Fern's scalar grammar name and exact source revision, build the API0.7 extension as a Preview2 component with a separate pinned toolchain, and stage a reproducible package without modifying editor profiles. Discover only the Rust language server or use explicit literal configuration.
* **Context**: The prior installer removed user profile directories, the manifest lacked grammar registration, and the documented extension artifact was absent. A successful grammar test alone did not prove extension loading or LSP startup. Zed's own binary.path override also bypasses extension-supplied arguments.
* **Consequences**: Explicit overrides include `arguments = ["lsp"]`. Extension Rust1.97.1 and wasm32-wasip2 remain separate from compiler MSRV1.75. Locked offline builds require provisioned dependencies. Package validation checks the complete component, nested API marker, pinned grammar bytes, four staged queries and reproducible archives; the installer becomes a staging wrapper. Actual Zed1.18.0 smoke tests use owned temporary profiles for both discovery and override modes. The project uses its portable Tree-sitter/SDK29 grammar: the separately reproduced Zed source-builder profile loads in native Zed but carries a libc dependency unavailable to the web test runtime. The official extension CLI and marketplace publication are not claimed. The grammar pin must be published before remote dev installation can fetch it. The former `editor/zed-fern/README.md` guide was retired with the integration in Decision122. The unavailable `/decision` skill is replaced by this established format.

The label/recovery follow-on pins grammar
`6d4efbb2f14a73be872f7c8e94c5ac31e54afcb9`. The stale revision fails exact staged
label-query validation; the matching revision passes reproducible packaging, the
85/33/30 corpus and both isolated actual-Zed startup modes with labeled source.
Remote publication remains outside this local verification.
