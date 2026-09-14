| Operation | Before | After | Rust | Before / after |
| --- | ---: | ---: | ---: | ---: |
| Scalar 20M | 75.47 ms | 85.38 ms | 57.49 ms | 0.88× |
| Source build | 44.32 ms | 44.27 ms | — | 1.00× |

Maximum measured peak RSS (bytes): scalar before/after/Rust [1572864, 1572864, 1589248]; compiler before/after [56836096, 57098240].

Whole-process medians exclude tagged warmups. Build timing includes code generation and system linking against the same runtime archive. All raw streams, timing samples, input hashes and fresh build hashes are retained.
