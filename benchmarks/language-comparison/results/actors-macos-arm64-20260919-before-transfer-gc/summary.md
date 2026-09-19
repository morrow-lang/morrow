| Workload | Schedulers | Implementation | Median process wall ms | Median ready-to-completion ms | Median operations/s | Minimum load-covered probe intervals | Median run p99 observed completion gap ms | Maximum run p99 observed completion gap ms |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| request-reply | 1 | morrow-pinned-r1 | 69.045 | 55.870 | 286381 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r1 | 69.058 | 55.241 | 289641 | 0 |  |  |
| request-reply | 1 | elixir-beam | 142.736 | 13.350 | 1198479 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r32-experimental | 69.070 | 55.316 | 289247 | 0 |  |  |
| contention | 1 | morrow-pinned-r1 | 448.931 | 436.885 | 588 | 256 | 0.009167 | 0.012959 |
| contention | 1 | morrow-stealing-r1 | 448.853 | 437.476 | 587 | 256 | 0.010875 | 0.019875 |
| contention | 1 | elixir-beam | 142.714 | 15.777 | 16289 | 53 | 0.274000 | 0.282250 |
| contention | 1 | morrow-stealing-r32-experimental | 455.518 | 443.816 | 579 | 256 | 0.044708 | 0.050583 |
| lifecycle | 1 | morrow-pinned-r1 | 18.965 | 9.148 | 83952 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r1 | 18.949 | 8.910 | 86192 | 0 |  |  |
| lifecycle | 1 | elixir-beam | 130.609 | 3.767 | 203894 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r32-experimental | 19.014 | 9.275 | 82800 | 0 |  |  |
| request-reply | 2 | morrow-pinned-r1 | 86.212 | 73.282 | 218335 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r1 | 1304.055 | 1292.466 | 12379 | 0 |  |  |
| request-reply | 2 | elixir-beam | 139.681 | 6.162 | 2596472 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r32-experimental | 484.299 | 458.978 | 34860 | 0 |  |  |
| contention | 2 | morrow-pinned-r1 | 490.106 | 466.788 | 551 | 256 | 0.017750 | 0.024625 |
| contention | 2 | morrow-stealing-r1 | 506.459 | 493.721 | 521 | 256 | 0.034166 | 0.170083 |
| contention | 2 | elixir-beam | 142.371 | 10.680 | 24063 | 256 | 0.037125 | 0.050375 |
| contention | 2 | morrow-stealing-r32-experimental | 459.279 | 435.922 | 590 | 256 | 0.038167 | 0.042166 |
| lifecycle | 2 | morrow-pinned-r1 | 18.942 | 7.253 | 105891 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r1 | 37.699 | 26.648 | 28820 | 0 |  |  |
| lifecycle | 2 | elixir-beam | 130.169 | 3.207 | 239504 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r32-experimental | 37.785 | 26.307 | 29194 | 0 |  |  |
| request-reply | 4 | morrow-pinned-r1 | 97.624 | 86.550 | 184865 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r1 | 1071.292 | 1044.967 | 15311 | 0 |  |  |
| request-reply | 4 | elixir-beam | 174.615 | 11.241 | 1423308 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r32-experimental | 592.185 | 571.460 | 27998 | 0 |  |  |
| contention | 4 | morrow-pinned-r1 | 493.045 | 466.841 | 551 | 256 | 0.018084 | 0.023792 |
| contention | 4 | morrow-stealing-r1 | 499.635 | 476.511 | 539 | 256 | 0.036542 | 0.162000 |
| contention | 4 | elixir-beam | 142.896 | 10.919 | 23537 | 256 | 0.030875 | 0.168459 |
| contention | 4 | morrow-stealing-r32-experimental | 469.853 | 443.233 | 580 | 256 | 0.037792 | 0.041583 |
| lifecycle | 4 | morrow-pinned-r1 | 18.964 | 7.268 | 105665 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r1 | 36.555 | 23.796 | 32274 | 0 |  |  |
| lifecycle | 4 | elixir-beam | 130.762 | 3.099 | 247822 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r32-experimental | 37.697 | 24.204 | 31731 | 0 |  |  |
