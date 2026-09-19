+++
schema_version = 1
id = "01M2XHZ8XNB6CGBTSQ46TCDT7R"
title = "Civetweb runtime backend for `http.get`/`http.post` (ship now)"
date = "2026-02-06"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Accepted
* **Decision**: I will implement Fern's HTTP runtime backend using vendored civetweb now, replacing placeholder `Err(FERN_ERR_IO)` behavior for successful HTTP requests.
* **Context**: The stdlib HTTP surface (`http.get`, `http.post`) was already stabilized in checker/codegen and only lacked runtime execution. We considered layered socket/parser composition versus a single dependency and prioritized the "best option now" for maturity, auditability, and delivery speed.
* **Consequences**: Runtime now performs real HTTP client requests via civetweb and returns `Ok(response_body)` on `2xx` responses; invalid URLs, non-`2xx` responses, and transport failures return `Err(FERN_ERR_IO)`. Civetweb v1.16 is vendored under `deps/civetweb`, runtime build/link paths include civetweb + pthread/OpenSSL requirements, and runtime-surface coverage includes local loopback GET/POST success tests (`tests/test_runtime_surface.c`). HTTPS/TLS is enabled in the runtime build.
