+++
schema_version = 1
id = "01M2XHZ89SCHC0NTVTJ407Z9E3"
title = "Ship the Rust compiler as fern with explicit native components"
date = "2026-09-12"
status = "accepted"
tags = ["rust", "runtime"]
supersedes = []
superseded_by = []
depends_on = ["01M2XHZ8EHT3630TCTPM7ZZDN6"]
related_to = []
+++
## Status

Accepted; default promotion verified on macOS/Linux arm64

## Decision

I will make the verified Rust frontend the default `fern`, retain the C frontend as `fern-c`, and distribute the QBE helper, native test supervisor and shared runtime beside them. A `fern-package.json` marker disables implicit development-checkout fallback for installed packages.

## Context

[Decision 96](2026-09-06_192327569_concentrate-new-language-features-in-rust-and-retain-c-as-a.md) authorizes the compiler migration after compatibility and platform gates. Replacing one executable without installing its required native helpers would produce a package that only works inside the checkout. Existing release recipes and installation tests describe the older C-only bundle.

## Consequences

Build, install, uninstall, archive and CI contracts must cover every component and execute real Rust-language applications outside the checkout. Missing helpers fail visibly even when a development checkout exists. Explicit component overrides remain supported. C bootstrap and legacy ABI tests remain separate required references. This compiler migration does not imply rewriting QBE, the runtime, supervisor or editor parser in Rust, or completing unrelated future language features. Switch defaults only after executable language/API/tooling acceptance, Result proofs and Linux/macOS release gates pass.
