+++
schema_version = 1
id = "01M2XHZ7YHXEETW5C9MZPYKA6P"
title = "Scan initialized actor payload roots and full-width message tags"
date = "2026-09-19"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Status

Adopted; padding regressions and integrated acceptance pass

## Decision

Store the message kind in a full-width `u64`, preserving the existing record offsets and size while initializing its entire scanned word. Group the actor's six payload pointers (`frame`, `selector`, `timeout_frame`, `first`, `last`, `scopes`) into one contiguous region, asserted at compile time. Actor and host-port constructors register exactly that region while retaining the original control allocation. Scheduler flags, alignment padding, descriptors and opaque Rust control ownership are not payload roots. Public Exec/Type/Function/Pid layouts, logical admission and heap-transfer rejection of actual foreign edges remain unchanged.

## Evidence and cause

The small-copy allocation experiment exposed a migration refusal in a retained concurrency test. Instrumentation traced ForeignEdge to the message kind word: seven padding bytes combined with the zero tag to equal a live foreign allocation address. The collector deliberately reads words through assembly; the demonstrated defect is a false graph edge, not an ordinary Rust uninitialized-load claim. An independent poisoned-message-word test reproduces the refusal before the full-width tag fix. Review found the same issue in the previous whole-Actor mutable root range; a separate poisoned-actor-padding test also fails with ForeignEdge before narrowing that range.

## Root and cleanup coverage

Six independently allocated two-block graphs remain live through precise collection, detach/adopt and another collection when stored in the six pointer fields; clearing the fields releases those graphs. Existing native monitor, cleanup, mailbox and continuation tests exercise their semantic contents. The concurrent migration test now releases and joins its blocked destination before asserting a transfer failure, so a failed oracle cannot leave a callback using expired stack storage. The legacy Supervisor record consists entirely of initialized word-sized fields and remains unchanged in this correction.

## Acceptance

The full macOS ARM64 gate passes 2,436 Rust tests across 295 suites, 317 native fixtures, 20 examples, 63 compatibility programs, 295 atomic rejections and 64+192+231 fuzz cases. The ordinary runtime has 230 tests; ThreadSanitizer passes 228 in 18.30 s, excluding the two long churn tests covered normally. Actor replay retains trace `0xd4e402a412f11e2f`, 48,739 callbacks and zero cleanup residue. Independent review verifies both root ranges, retained control bases and the test graphs; 36 release benchmark semantic smoke checks pass.

## Performance boundary

This is a correctness correction required before evaluating the copy optimizations. Allocation-count reductions and fewer scanned control words are not throughput evidence; rebuild the baseline and measure the separate candidate.
