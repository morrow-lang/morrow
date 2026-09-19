| Workload | Schedulers | Implementation | Median process wall ms | Median ready-to-completion ms | Median operations/s | Minimum load-covered probe intervals | Median run p99 observed completion gap ms | Maximum run p99 observed completion gap ms |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| request-reply | 1 | morrow-pinned-r1 | 55.618 | 45.016 | 355430 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r1 | 59.805 | 45.831 | 349106 | 0 |  |  |
| request-reply | 1 | elixir-beam | 146.746 | 13.929 | 1148662 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r32-experimental | 55.125 | 43.959 | 363975 | 0 |  |  |
| contention | 1 | morrow-pinned-r1 | 30.704 | 21.260 | 12089 | 256 | 0.010042 | 0.021083 |
| contention | 1 | morrow-stealing-r1 | 31.172 | 21.127 | 12165 | 256 | 0.009500 | 0.013584 |
| contention | 1 | elixir-beam | 161.365 | 16.582 | 15499 | 52 | 0.286209 | 0.362083 |
| contention | 1 | morrow-stealing-r32-experimental | 31.115 | 19.236 | 13360 | 256 | 0.014916 | 0.019875 |
| lifecycle | 1 | morrow-pinned-r1 | 18.963 | 9.046 | 84904 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r1 | 18.737 | 9.027 | 85080 | 0 |  |  |
| lifecycle | 1 | elixir-beam | 135.970 | 4.018 | 191162 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r32-experimental | 18.952 | 9.078 | 84605 | 0 |  |  |
| request-reply | 2 | morrow-pinned-r1 | 60.960 | 47.105 | 339666 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r1 | 87.527 | 75.918 | 210754 | 0 |  |  |
| request-reply | 2 | elixir-beam | 144.260 | 7.217 | 2217013 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r32-experimental | 71.688 | 62.190 | 257275 | 0 |  |  |
| contention | 2 | morrow-pinned-r1 | 37.249 | 27.134 | 9471 | 256 | 0.016917 | 0.064042 |
| contention | 2 | morrow-stealing-r1 | 41.096 | 29.534 | 8702 | 256 | 0.017666 | 0.031875 |
| contention | 2 | elixir-beam | 145.958 | 11.023 | 23314 | 256 | 0.027459 | 0.058125 |
| contention | 2 | morrow-stealing-r32-experimental | 31.203 | 18.314 | 14033 | 256 | 0.019583 | 0.043250 |
| lifecycle | 2 | morrow-pinned-r1 | 19.014 | 7.079 | 108488 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r1 | 37.410 | 26.914 | 28535 | 0 |  |  |
| lifecycle | 2 | elixir-beam | 135.586 | 3.198 | 240156 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r32-experimental | 37.772 | 26.798 | 28659 | 0 |  |  |
| request-reply | 4 | morrow-pinned-r1 | 74.488 | 49.487 | 323316 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r1 | 87.198 | 64.123 | 249521 | 0 |  |  |
| request-reply | 4 | elixir-beam | 148.349 | 10.146 | 1576905 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r32-experimental | 73.311 | 51.568 | 310272 | 0 |  |  |
| contention | 4 | morrow-pinned-r1 | 36.956 | 27.203 | 9448 | 256 | 0.016334 | 0.019917 |
| contention | 4 | morrow-stealing-r1 | 37.772 | 27.782 | 9250 | 256 | 0.019791 | 0.023833 |
| contention | 4 | elixir-beam | 145.975 | 11.058 | 23242 | 125 | 0.103083 | 0.178375 |
| contention | 4 | morrow-stealing-r32-experimental | 31.512 | 18.263 | 14072 | 256 | 0.021041 | 0.141333 |
| lifecycle | 4 | morrow-pinned-r1 | 18.964 | 7.036 | 109153 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r1 | 37.788 | 26.477 | 29006 | 0 |  |  |
| lifecycle | 4 | elixir-beam | 135.463 | 3.349 | 229336 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r32-experimental | 37.164 | 25.465 | 30159 | 0 |  |  |
