| Operation | Before | After | Rust | Before / after |
| --- | ---: | ---: | ---: | ---: |
| Scalar 20M | 81.60 ms | 80.41 ms | 57.25 ms | 1.01× |
| Source build | 42.97 ms | 44.67 ms | — | 0.96× |

Maximum measured peak RSS (bytes): scalar before/after/Rust [1572864, 1572864, 1589248]; compiler before/after [57049088, 56770560].

Whole-process medians exclude tagged warmups. Build timing includes code generation and system linking against the same runtime archive. All raw streams, timing samples, input hashes and fresh build hashes are retained.
