+++
schema_version = 1
id = "01M2XHZ92QS6WT2NGF03VXPXED"
title = "Deterministic simulation testing for actors (FernSim)"
date = "2026-01-28"
status = "accepted"
tags = ["testing", "runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will implement deterministic simulation testing (FernSim) for the actor runtime and supervision trees, inspired by TigerBeetle's VOPR and FoundationDB's simulation testing.

## Context

To achieve BEAM-level reliability for Fern's actor system, real-world testing is insufficient - it would take years to hit rare edge cases. Deterministic simulation can explore millions of process scheduling interleavings, inject faults (crashes, timeouts, message loss), and reproduce any bug with a seed. TigerBeetle found critical bugs in 3 weeks that would have taken 5+ years to find in production. FoundationDB credits simulation testing for their legendary reliability.

## Consequences

FernSim will be a core part of Milestone 8 (Actor Runtime). The actor scheduler must support both real execution and simulated execution with a deterministic PRNG. All supervision strategies (one_for_one, one_for_all, rest_for_one) will be verified against fault injection. CI will run simulation tests on every PR. Success criteria: 1M+ simulated steps with zero invariant violations before release.
