Status at pause: verified isolated candidate, **not applied to main and not measured**.
The durable patch is `candidate.patch`; use the evidence files in this directory.

# Memo-only copy candidate — 2026-09-19

Worktree: `/tmp/morrow-copy-memo-only-20260919`, detached cafc91d. Reused isolated target `/tmp/morrow-unboxed-send-target-20260919` after its previous task released ownership. Patch: `/tmp/morrow-copy-memo-only-20260919.patch`; SHA256 `fd0b9d311d9bc407111f5bdbfe49585970479c2f6eecd2100c9efec7c764479e`. No main/OTP changes, commits, benchmark processes or timing claims.

The combined tiny-fragment+memo candidate was deferred after mixed paired diagnostics. This candidate isolates only the temporary copy memo; the measurements do not identify a causal culprit in the combined candidate.

## Production scope

Only `managed/copy.rs` and new `managed/copy_memo.rs`: replace `Copy.seen` HashMap with a safe two-entry inline enum and unchanged HashMap overflow. Keys stay exact `(i64 source, usize descriptor)`; values remain full-width i64. Replacing an existing key happens before considering spill; `(0,0)` is a valid stored key, tracked by length rather than a sentinel. Third distinct key spills once and retains both earlier entries. No changes to traversal, preflight, native allocation, roots, controls, retention or quotas.

The entire descriptor-dispatch `Copy::value` body, including PID, ProcessId, MonitorRef and JSON arms, and the JSON-copying methods were independently compared byte-for-byte with cafc91d and match. JSON's separate HashMap remains unchanged. `memory.rs` is unchanged (SHA256 `0b63d138f23c0c0a0802599679b71651f94448139fa69d0f7d16e76b4069f0c2`); `memory/fragment.rs` is unchanged (SHA256 `144842419353da56a5092cfcdb11a93d51ccd2ca2b0486d72d9a4af57ad7f035`). There is no tiny-fragment storage/adoption code in this patch.

Memo is64 bytes versus prior HashMap48 (+16 transient stack bytes). FragmentCopy remains112 bytes; the larger combined candidate's fragment layout is absent. No new production unsafe or dependencies.

## Fresh red → green

The tests were added to the cafc91d implementation before replacing HashMap. Actual native `value_fragment` copies of a one-block scalar record and a two-block owned-string record each allocated2 plain metadata blocks,916 bytes. The expected two combined metadata allocations failed: actual4 versus expected2. Inline replacement/all-zero-key test also failed with1 allocation versus0. Four semantic tests already passed. Full red log preserved.

After the Memo-only implementation, each actual graph copy allocates1 plain block,808 bytes: **one temporary metadata allocation and108 bytes removed per tiny copy**, while unchanged Fragment metadata still allocates. Native payload allocations stay exactly1/2 for the corresponding graphs, with distinct source/destination addresses and exact values. All six focused tests pass. Do not describe this candidate as removing all metadata allocation.

Independent oracles cover actual source/descriptor-sensitive graph identity, full-width values and descriptor0 frame key, actual `(0,0)` replacement before and after spill, retained DAG sharing across spill/adoption/precise collection,64-string heap-copy fallback rooted through collection, and cyclic preflight rejection before allocation. A test-only forwarding global allocator records actual plain/zeroed allocations on the calling thread; it does not change production allocation.

## Verification

All commands ran from the isolated worktree with `rtk env CARGO_TARGET_DIR=/tmp/morrow-unboxed-send-target-20260919`:

- `cargo test -p morrow-runtime --lib memo_tests -- --nocapture`: red4passed/2failed; green6passed. Logs `red` and `green` retain exact allocation breakdowns.
- `cargo test -p morrow-runtime`: **236 unit +19 integration/protocol tests passed**; existing JSON, ProcessId/MonitorRef, GC, heap transfer, mailbox, quota, cancellation and lifecycle tests remain unchanged. Log `runtime-all`.
- `cargo build -p morrow-runtime`: produced the isolated core static archive used by the native oracle.
- `cargo test -p morrow --test process_model_native`: **2 tests,12 native process executions** across schedulers1/2/4 and stealing0/1; exact output and empty stderr, explicit precise collections, copied identities/monitors and full-width messages passed. Log `process-native`.
- `cargo clippy -p morrow-runtime --all-targets -- -D warnings`: passed. Log `clippy`.
- Targeted rustfmt and `git diff --check`: passed.

Parent owns integration gate and any future quiet comparison. The patch establishes the bounded allocation effect and semantic invariants; whether it improves workload throughput is unmeasured.
