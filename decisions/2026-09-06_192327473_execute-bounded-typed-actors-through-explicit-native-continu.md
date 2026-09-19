+++
schema_version = 1
id = "01M2XHZ8BHXWDBTEEPVKPN6TH3"
title = "Execute bounded typed actors through explicit native continuations"
date = "2026-09-06"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for native105A; generalized suspension and supervision remain open
* **Decision**: I will use invocation-owned cooperative actors with invariant typed mailboxes, selective receive, one-time monotonic deadlines and compiler-owned continuation frames. Keep ordinary environment/fault ABI unchanged and pass a separate execution context to managed calls; native callback status and payload are int64/QBE `l`.
* **Context**: Rust actor syntax must execute with defined ownership and scheduling rather than remain a type-checking placeholder. Source/native tests and independent runtime/public-IR probes exposed descriptor-budget, timestamp, root-retirement and pre-lowering validation defects, which require fixes before publication. The unavailable `/decision` skill is replaced by this established format.
* **Consequences**: Validate original and inactive public IR before conversion erases type evidence; cap generated identities. Spawn queues work, send borrows values and returns Result(Unit,Int), and locally owned Result duties survive suspension without handling credit. Retire spent roots and preserve unmatched messages under explicit live/identity/mailbox/retained-byte/work limits. Timely queued matches remain eligible after delayed polling; late or equal-deadline arrivals cannot defeat timeout. Receiving-owned defer, non-tail/indirect suspension, typed supervision and REPL/FernSim execution remain explicitly unsupported. Existing C actor/lifecycle APIs retain their separate contract. See [the complete source, ABI, quota and failure policy](../docs/RUST_ACTORS.md).

Phase measurements found redundant actor preparation in ordinary programs. Preserve every original public-IR check, reuse immutable validated layouts/effects, and borrow the program when no continuation functions are appended. Revalidate the combined tree when functions are added. Select byte-identical ordinary or actor fault support. The [paired Criterion record](../benchmarks/compiler-phases/actor105-preparation-review.json) documents the measured recovery and remaining cost; shared-host phase timings are not end-to-end or native execution guarantees.
