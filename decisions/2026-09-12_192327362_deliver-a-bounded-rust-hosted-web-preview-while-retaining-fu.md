+++
schema_version = 1
id = "01M2XHZ882DG9GHZBAYY93GFAR"
title = "Deliver a bounded Rust-hosted web preview while retaining full Fern application gates"
date = "2026-09-12"
status = "accepted"
tags = ["rust"]
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2XHZ889SVS560KRJ6YHDSZK"]
+++
## Status

✅ Adopted; preview foundations implemented

## Decision

Deliver the first collaborative checklist with a real compiled Fern WebAssembly policy module, a Rust browser host and service worker, a portable bounded command/snapshot protocol and an authenticated Rust HTTP/WebSocket server. Embed generated browser assets and dependency notices in the executable, with Linux musl targets for static deployment. Preserve the distinction between this working preview and the complete Fern domain-actor/model/update/view architecture in [Decision 124](2026-09-12_192327369_prioritize-supervised-native-actors-and-reactive-fern-webass.md).

## Context

The separate WASM emitter now consumes checked semantic IR, preserving i64 integers and supporting scalar values plus a bounded precise String heap. Native runtime work adds actor-owned payload heaps, copied message/capture graphs and explicit compiler root frames; conservative scanning remains. The browser can execute Fern policy, update keyed accessible DOM through Rust, keep local drafts and reload cached confirmed state offline. The preview's server domain model and client model storage remain Rust. A serialized Rust owner task does not establish fairness or supervision for compiled Fern actors.

## Consequences

Generated JavaScript bindings and loading glue remain build artifacts; authored implementation stays Rust and Tree-sitter stays removed. Ordinary CLI programs do not depend on the web packages. The ephemeral protocol distinguishes resource incarnations, resumable namespaces and physical connections, bounds retained outcomes and never blindly replays uncertain mutations into reset state. Real browser checks cover two clients, task changes, filtering, focus, offline reload, drafts, reconnect and logout against an ARM64 static server running unprivileged in an empty Linux chroot. x86-64 static ELF validation does not establish x86-64 execution. Complete typed Fern UI/server integration, precise native root/layout coverage, resumable fairness, typed supervision, multicore scheduling, durability and clustering remain separate gates. See the [preview guide](../docs/WEB_PREVIEW.md), [architecture](../docs/FULL_STACK_ARCHITECTURE.md) and [roadmap](../ROADMAP.md); the earlier native migration report does not validate these new components.
