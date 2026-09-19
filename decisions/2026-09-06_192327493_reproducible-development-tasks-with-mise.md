+++
schema_version = 1
id = "01M2XHZ8C5CX1RJ31TATKP1D95"
title = "Reproducible development tasks with mise"
date = "2026-09-06"
status = "accepted"
tags = ["tooling"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Adopted
* **Decision**: I will use mise as the maintained development environment and task runner, preserving Rust 1.75 and isolating optional newer developer tools.
* **Context**: The user requested reproducible onboarding and broad adoption of the Rust review guidance without Nix/devenv.
* **Consequences**: Adopt mise as the sole maintained task/environment entry point, replacing Justfile and mask. Keep existing task names and native gate commands; run composite clean/build/consumer steps sequentially, and set default task jobs to one. No Nix/devenv configuration is introduced.

Pin the required tools to Rust1.75.0 (rustfmt, Clippy, rust-src), Python3.14.7 (the existing CPython3.14 reference contract) and uv0.12.5. CI pins mise2026.9.1 and immutable mise-action commit c2a87611a18de5b3828c5652fe268e992400cb5c. Mise configuration accepts this version or newer. Pin binary-download URLs and SHA256 values on Linux/macOS x64/arm64; use strict config-scoped locking. Rust remains a rustup-backed version pin verified by its distribution mechanism, not a mise URL lock. The three Python reference-script graphs have uv script locks and enforced --locked execution; stale metadata fails without running scripts or updating locks. Native packages/SDKs remain host-managed inputs, not a reproducible OS image or offline build claim.

Centralize authored C configuration in scripts/build_config, consumed by task scripts and the existing bounded native checker bootstrap. Keep its source snapshot, compiler-profile rejection, content/dependency cache validation and supervisor lifecycle unchanged. The checker itself does not execute Python/Cargo; mise may provision configured project tools before any task. Direct scripts/check_style execution remains available with only native dependencies.

Expose focused non-writing Rust formatting, locked all-target/all-feature checking, Clippy warnings-as-errors, tests and documentation tests. The editor baseline uses an explicitly installed Rust Analyzer editor extension plus rust-src; no extension is installed silently. Preserve Zed package Rust1.97.1 through a task-local process scope so the project Rust1.75 environment cannot override its component toolchain.

Provide opt-in nextest0.9.143 (prebuilt, four threads, no retries) and watchexec2.7.1 (explicit project origin, literal queued cargo-check command). Required CI retains cargo test and documentation tests. Provide optional Bacon3.25.0 check/Clippy UI: its installer uses a separately pinned Rust1.98.1 and cargo install --locked into compiler-rs/target/dev-tools/bacon-3.25.0; its actual project jobs explicitly select Rust1.75. No global executable/default or application dependency changes. This source build and extra compiler are opt-in, not ordinary setup requirements.

Keep historical decisions and published measurements unchanged. Update active guidance, CI, release/developer scripts, compiler help and bootstrap workflow to mise. Cargo-generate/cargo-seek have no concrete existing workflow here and are not installed speculatively. Decision107 supplies the verified rust-lint-policy, rust-bench-smoke and rust-bench tasks and a separate developer dependency lock.

Verification includes real runner dependency serialization, literal source/config/installation paths, fail-fast native task dispatch, pin mismatch rejection, all platform lock records, optional toolchain separation, and the existing bootstrap/native workflow gates. Mise provisioning installs the configured rust-src component for the pinned project toolchain. Fresh Linux tools and the combined macOS/Linux Rust, native, cache, documentation and nextest gates passed. Literal quoted build flags and pkg-config paths use a bounded non-evaluating decoder, tested across all build helpers and generated roundtrips.
