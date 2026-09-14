# Immutable callback optimization, 2026-09-14

Fern's unchanged immutable model workload is **2.73–2.94× faster** after removing
repeated tree updates from native GC frame registration. Lists still map every
element into a fresh collection and share unchanged records. Retained versions
remain usable; this does not introduce mutation into Fern programs.

| Whole-process workload | Before | After | Optimized Rust | Fern speedup |
| --- | ---: | ---: | ---: | ---: |
| 10,000 updates × 256 entries | 70.55 ms | 25.87 ms | 3.49 ms | 2.73× |
| 100,000 updates × 256 entries | 676.18 ms | 230.00 ms | 12.22 ms | 2.94× |

Median peak RSS at 10,000 updates was **3.45 MiB before / 3.47 MiB after**;
at 100,000 updates it was **3.47 / 3.48 MiB**. This is a speed improvement with
similar measured memory usage. Rust used 1.55 MiB. Fern still trails Rust on this
workload, especially once startup costs are amortized. Bun was not rerun here;
the [earlier three-language experiment](README.md) remains historical evidence.

## Attribution and implementation

A five-second macOS sample of the original model running five million updates
showed most sampled time under callback frame entry/exit. Every record callback
inserted into, then removed from, two BTreeMaps: one for its token and another
for its heap's root range. The 10,000-update workload invokes 2.56 million such
callbacks even though only 10,000 records change.

Native frames now use reusable vector storage with monotonically issued tokens
and their originating heap identity. Ordinary call/return uses push/pop;
out-of-order or stale exits search the active frames. Collection streams the
current heap's registered ranges directly, without copying all root words.
Persistent Rust-container roots retain their existing registration mechanism.
Heap retirement removes its outstanding frames before their slots can disappear.

The first vector implementation measured 29.01 ms at 10,000 updates. Its profile
still showed a zero-length `memmove` from `Vec::remove` on ordinary exits;
explicitly popping the last frame produced the final implementation above.
[Intermediate measurements and both profiles](results/immutable-macos-arm64-20260914/)
are retained, alongside the [final raw samples, binary and input hashes](results/immutable-macos-arm64-20260914-final/).
Sampling diagnoses where execution spends time; it is separate from uninstrumented
timing. The original profile used the earlier `workloads-fern` executable, while
the paired timing rebuilds the same source against the frozen old runtime.

This changes runtime bookkeeping only. Compiler settings, generated collection
loops, actor continuation boundaries, full-width payloads and checked fault paths
are unchanged. Cranelift still uses `opt_level=none`. A frame's normal entry/exit
is amortized O(1); unusual out-of-order exits are O(active frame depth), compared
with the previous tree lookup. Registry capacity follows peak active depth and
is released at invocation shutdown. This is not a persistent-vector or
copy-on-write implementation, and does not establish actor/server throughput.

## Correctness and resource checks

* An independent allocator counter observes **16,128 allocations before and zero
  after** across 4,096 callback cycles after warming to depth 32. The assertion
  failed before implementation. It measures allocation behavior, not elapsed time.
* **8,192 seeded lifecycle operations** exercise nesting, cross-heap exits,
  repeated/stale tokens, precise collection and retirement against finalizer state.
  Separate ABI checks cover updated/cleared slots, empty ranges, persistent roots
  and exclusion of other heaps' frames.
* Native generated-code tests retain **600 historical records** across three
  seeds while forcing precise collection in nested captured maps. Independent
  expected output checks i64 extremes, values above 2^53, floats, Unicode strings
  and nested tuples/lists. A separate oracle checks the original fault, deferred
  cleanup order, complete reclamation and a subsequent successful invocation.
  These semantic tests also pass against the frozen old runtime.
* Every compared executable passes **49 independent checksum/retained-original
  cases**, including empty, boundary-cycle and alternate-seed inputs. All timed
  outputs are checked too. The oracle counts visits using a modular inverse,
  independently of the measured map loop.

The complete macOS `cargo xtask check` passed: **2,183 Rust tests**, 311 native
fixtures, 20 examples, 63 dynamic compatibility programs, 295 atomic rejections
and 64+192+231 fuzz cases, with formatting, notices and Clippy checks.

## Measurement and reproduction

Apple M4, 24 GiB RAM, macOS 26.5.1 ARM64; pinned Rust
`1.100.0-nightly (f248f4038 2026-09-05)`. The parent source is `9b7d3a0`;
the changed runtime is the implementation accompanying this report. Both Fern
executables use the same release compiler and release runtime build profile.
Rust is the retained `-O -C strip=symbols -C panic=abort` comparator. The laptop
had ordinary desktop applications open; project builds/tests did not overlap
timing. Nine rotated rounds follow one full-workload warmup. All 60 samples,
including warmups, are retained. Timing includes `/usr/bin/time -l` launch and
reaping; RSS is that tool's maximum resident size in bytes. No timing threshold
is enforced in CI, and small differences should not be generalized.

Use separate checkouts to build the old (`9b7d3a0`) and current release compiler
and native runtime with `cargo build --release -p fern -p fern-runtime-native`.
In each checkout, set `FERN_RUNTIME_LIB` to its absolute
`target/release/libfern_runtime_native.a` path and build the unchanged
`benchmarks/language-comparison/programs/workloads.fn` to a distinct executable.
Then, on macOS:

```sh
rustc --edition=2024 -Dwarnings benchmarks/language-comparison/src/immutable.rs -o /tmp/fern-immutable-runner
rustc --edition=2024 -O -C strip=symbols -C panic=abort benchmarks/language-comparison/programs/workloads.rs -o /tmp/fern-model-rust
/tmp/fern-immutable-runner /absolute/before /absolute/after /tmp/fern-model-rust /tmp/new-immutable-results
```

The output directory must not already exist. Build everything before timing;
retain the two source revisions and archive hashes with the result. Run
`cargo test -p fern-runtime` and `cargo test -p fern --test cranelift_backend immutable_gc`
for focused correctness, followed by `cargo xtask check` for the repository gate.
