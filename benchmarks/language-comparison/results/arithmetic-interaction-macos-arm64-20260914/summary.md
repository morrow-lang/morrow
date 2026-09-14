| Operation | Before | After | Rust | Before / after |
| --- | ---: | ---: | ---: | ---: |
| Scalar 20M | 75.74 ms | 66.24 ms | 57.60 ms | 1.14× |
| Source build | 42.35 ms | 43.96 ms | — | 0.96× |

Maximum measured peak RSS (bytes): scalar before/after/Rust [1572864, 1572864, 1589248]; compiler before/after [56770560, 56508416].

Whole-process medians exclude tagged warmups. Build timing includes code generation and system linking against the same runtime archive. All raw streams, timing samples, input hashes and fresh build hashes are retained.
