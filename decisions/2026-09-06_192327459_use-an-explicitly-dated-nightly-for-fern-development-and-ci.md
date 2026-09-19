+++
schema_version = 1
id = "01M2XHZ8B3JWM8BX02GFDE73YM"
title = "Use an explicitly dated nightly for Fern development and CI"
date = "2026-09-06"
status = "accepted"
tags = ["ci"]
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2XHZ8VBJ09WNWNV74SD6TAR", "01M2XHZ8C5CX1RJ31TATKP1D95", "01M2XHZ8BYB69321D49Y11S2RP", "01M2XHZ8CDWS46QYZGXCARGQ6W"]
+++
## Status

Accepted; supersedes the Rust1.75 preservation policy in Decisions [45](2026-09-05_192327979_evaluate-a-safe-rust-frontend-with-typed-ir-and-the-existing.md)/[106](2026-09-06_192327493_reproducible-development-tasks-with-mise.md)/[107](2026-09-06_192327486_incremental-rust-lint-and-benchmark-guidance.md)/[109](2026-09-06_192327501_reevaluate-native-backends-with-measured-user-workflows.md)

## Decision

I will use nightly-2026-09-06, with its rustfmt, Clippy and rust-src, through both mise and a matching root rust-toolchain.toml. The user explicitly requested nightly; do not retain an unsupported Rust1.75 compatibility promise.

## Context

The official nightly manifest provides Rust1.100.0-nightly and the required components on Linux/macOS arm64/x86-64. A date pin preserves repeatable tool selection while allowing deliberate future upgrades. Current Cranelift does not require nightly, but the new policy removes the old frontend-toolchain obstacle. The unavailable `/decision` skill is replaced by this established format.

## Consequences

Keep edition2021 and standard-library-only production dependencies. Cargo's numeric rust-version1.100 is a minimum version check, not a claim that an untested stable compiler is supported; the dated nightly is the tested build policy. Compare active compiler, Cargo, formatter and Clippy identities with the installed pinned toolchain, require rust-src, and test pin drift. Migrate renamed lints with real negative fixtures and fix new diagnostics without broad policy suppression. A local large_enum_variant allowance preserves the public statement IR; boxing it requires a separate allocation/layout audit. Bacon's jobs follow Fern nightly; its installer and the Zed component retain their independent stable pins. Preserve historical benchmark evidence. Revalidate native behavior, cleanup, docs and developer tools before recording this migration complete. Nightly adoption does not itself implement or select a Cranelift backend.
