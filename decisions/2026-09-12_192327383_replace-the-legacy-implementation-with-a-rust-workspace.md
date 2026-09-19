+++
schema_version = 1
id = "01M2XHZ88QPXRDPM02A0BCS1VD"
title = "Replace the legacy implementation with a Rust workspace"
date = "2026-09-12"
status = "accepted"
tags = ["rust"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted; Rust workspace and release artifacts verified on macOS/Linux ARM64
* **Decision**: Replace all Fern-owned implementation with Rust, use Cranelift as the native backend, remove the C reference compiler and QBE implementation, and organize the project as a Cargo workspace.
* **Context**: The compiler-default migration deliberately retained native C components. The user now explicitly requests a wholly Rust implementation and removal of the old setup, superseding those retention requirements. The user explicitly chose removal of Tree-sitter with retention of the Rust LSP. Rust wrappers around third-party native libraries are allowed, with a preference for native Rust dependencies. Preserve SQLite behavior through its Rust wrapper rather than substituting an incompatible database.
* **Consequences**: Preserve executable language behavior and independent expected-output tests. Use a real Rust-owned nonmoving collector, not allocations retained until process exit; constrain and document unsafe native ABI/OS boundaries. Port process cleanup, quotas, codecs and package validation before retiring their implementations. Replace C-specific style and bootstrap machinery with Rust quality checks. Generated project documentation uses a static accessible module index and browser Find, superseding the authored JavaScript filter in Decision73. Keep historical decisions and measurement records visibly historical; the previous default-migration acceptance does not establish acceptance of this new runtime/backend. Full debug workspace/native gates and selected optimized runtime/ABI boundaries now pass on macOS/Linux ARM64. Both platform archives also pass actual relocated build/run/source-test and installation/uninstall checks, with matching artifact/performance hashes. Exact scope, counts, remaining architecture limits and final artifact evidence are recorded in [Rust workspace acceptance](../docs/RUST_WORKSPACE.md).
