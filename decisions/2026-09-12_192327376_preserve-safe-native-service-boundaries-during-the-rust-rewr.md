+++
schema_version = 1
id = "01M2XHZ88GNRBFQ1XBDF68TG53"
title = "Preserve safe native service boundaries during the Rust rewrite"
date = "2026-09-12"
status = "accepted"
tags = ["rust", "architecture"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Adopted
* **Decision**: Use certificate-verifying ureq/rustls for HTTP, rusqlite for SQLite compatibility, and explicit resource bounds for native text/process/TUI operations. Prefer native Rust while permitting Rust-wrapped third-party native dependencies as authorized.
* **Context**: Reproducing a native buffer overflow or accepting an untrusted certificate is not a useful compatibility requirement. Existing ordinary outputs and error domains remain covered by independent fixtures. The retired C-only string-copy JSON ABI is removed; source JSON uses the validated opaque shared Rust engine.
* **Consequences**: HTTP rejects invalid certificates, redirects, non-2xx status and invalid text, with a 30-second deadline and 16 MiB response cap. TUI rendering is capped at 16 MiB and negative padding is clamped. Both process argv paths enforce 4096 arguments and 1 MiB valid UTF-8 input. These bounded corrections are explicit; the rewrite does not promise to reproduce unsafe or unbounded old behavior. The old 4 MiB frontend budget measured a compiler without embedded Cranelift; new reports identify exact compiler/runtime hashes and do not reuse its measurements.
