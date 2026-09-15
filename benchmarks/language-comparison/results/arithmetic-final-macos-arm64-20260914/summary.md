> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

| Operation | Before | After | Rust | Before / after |
| --- | ---: | ---: | ---: | ---: |
| Scalar 20M | 76.60 ms | 66.57 ms | 57.35 ms | 1.15× |
| Source build | 43.00 ms | 42.48 ms | — | 1.01× |

Maximum measured peak RSS (bytes): scalar before/after/Rust [1572864, 1572864, 1589248]; compiler before/after [56721408, 56737792].

Whole-process medians exclude tagged warmups. Build timing includes code generation and system linking against the same runtime archive. All raw streams, timing samples, input hashes and fresh build hashes are retained.
