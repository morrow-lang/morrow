| Workload | Schedulers | Implementation | Median process wall ms | Median ready-to-completion ms | Median operations/s | Minimum load-covered probe intervals | Median run p99 observed completion gap ms | Maximum run p99 observed completion gap ms |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| request-reply | 1 | morrow-pinned-r1 | 61.883 | 52.226 | 306360 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r1 | 62.706 | 53.360 | 299849 | 0 |  |  |
| request-reply | 1 | elixir-beam | 148.290 | 13.983 | 1144216 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r32-experimental | 62.251 | 52.279 | 306049 | 0 |  |  |
| contention | 1 | morrow-pinned-r1 | 56.271 | 43.313 | 5934 | 256 | 0.011875 | 0.021208 |
| contention | 1 | morrow-stealing-r1 | 56.448 | 43.140 | 5957 | 256 | 0.011583 | 0.013084 |
| contention | 1 | elixir-beam | 150.854 | 16.102 | 15961 | 52 | 0.283583 | 0.321500 |
| contention | 1 | morrow-stealing-r32-experimental | 50.303 | 41.436 | 6202 | 256 | 0.013166 | 0.021625 |
| lifecycle | 1 | morrow-pinned-r1 | 19.027 | 9.449 | 81281 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r1 | 19.002 | 9.348 | 82157 | 0 |  |  |
| lifecycle | 1 | elixir-beam | 142.723 | 5.305 | 144763 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r32-experimental | 19.009 | 9.369 | 81970 | 0 |  |  |
| request-reply | 2 | morrow-pinned-r1 | 68.102 | 53.917 | 296752 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r1 | 99.155 | 86.012 | 186020 | 0 |  |  |
| request-reply | 2 | elixir-beam | 142.723 | 6.059 | 2640754 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r32-experimental | 75.318 | 66.869 | 239272 | 0 |  |  |
| contention | 2 | morrow-pinned-r1 | 61.361 | 51.834 | 4958 | 256 | 0.014542 | 0.033667 |
| contention | 2 | morrow-stealing-r1 | 67.995 | 56.994 | 4509 | 256 | 0.014166 | 0.035792 |
| contention | 2 | elixir-beam | 148.216 | 10.883 | 23615 | 118 | 0.020500 | 4.744292 |
| contention | 2 | morrow-stealing-r32-experimental | 50.180 | 39.898 | 6441 | 256 | 0.022959 | 0.043250 |
| lifecycle | 2 | morrow-pinned-r1 | 18.982 | 7.385 | 103998 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r1 | 50.316 | 38.148 | 20132 | 0 |  |  |
| lifecycle | 2 | elixir-beam | 131.480 | 3.388 | 226694 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r32-experimental | 50.301 | 36.930 | 20796 | 0 |  |  |
| request-reply | 4 | morrow-pinned-r1 | 68.184 | 56.842 | 281482 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r1 | 80.622 | 58.235 | 274749 | 0 |  |  |
| request-reply | 4 | elixir-beam | 151.473 | 10.204 | 1567942 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r32-experimental | 75.266 | 52.771 | 303199 | 0 |  |  |
| contention | 4 | morrow-pinned-r1 | 61.907 | 51.287 | 5011 | 256 | 0.010792 | 0.013333 |
| contention | 4 | morrow-stealing-r1 | 62.736 | 52.806 | 4867 | 256 | 0.011583 | 0.019542 |
| contention | 4 | elixir-beam | 142.261 | 10.750 | 23906 | 256 | 0.025375 | 0.044750 |
| contention | 4 | morrow-stealing-r32-experimental | 50.001 | 38.908 | 6605 | 256 | 0.016625 | 0.017542 |
| lifecycle | 4 | morrow-pinned-r1 | 18.980 | 8.146 | 94279 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r1 | 30.263 | 18.106 | 42418 | 0 |  |  |
| lifecycle | 4 | elixir-beam | 142.742 | 4.069 | 188754 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r32-experimental | 29.026 | 16.589 | 46297 | 0 |  |  |
