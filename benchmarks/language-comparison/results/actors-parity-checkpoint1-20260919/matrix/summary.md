| Workload | Schedulers | Implementation | Median process wall ms | Median ready-to-completion ms | Median operations/s | Minimum load-covered probe intervals | Median run p99 observed completion gap ms | Maximum run p99 observed completion gap ms |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| request-reply | 1 | morrow-pinned-r1 | 67.414 | 55.048 | 290655 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r1 | 68.843 | 54.379 | 294229 | 0 |  |  |
| request-reply | 1 | elixir-beam | 148.890 | 14.169 | 1129239 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r32-experimental | 68.916 | 54.965 | 291093 | 0 |  |  |
| contention | 1 | morrow-pinned-r1 | 222.051 | 210.770 | 1219 | 256 | 0.012417 | 0.014958 |
| contention | 1 | morrow-stealing-r1 | 223.198 | 210.022 | 1224 | 256 | 0.012500 | 0.013917 |
| contention | 1 | elixir-beam | 152.020 | 16.359 | 15710 | 53 | 0.285500 | 0.294500 |
| contention | 1 | morrow-stealing-r32-experimental | 223.598 | 212.196 | 1211 | 256 | 0.044042 | 0.055041 |
| lifecycle | 1 | morrow-pinned-r1 | 18.940 | 9.362 | 82034 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r1 | 18.945 | 9.318 | 82420 | 0 |  |  |
| lifecycle | 1 | elixir-beam | 136.557 | 4.093 | 187620 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r32-experimental | 18.977 | 9.266 | 82882 | 0 |  |  |
| request-reply | 2 | morrow-pinned-r1 | 69.064 | 55.339 | 289125 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r1 | 149.979 | 138.708 | 115350 | 0 |  |  |
| request-reply | 2 | elixir-beam | 137.459 | 6.125 | 2612263 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r32-experimental | 144.129 | 131.860 | 121341 | 0 |  |  |
| contention | 2 | morrow-pinned-r1 | 248.856 | 224.720 | 1144 | 256 | 0.017166 | 0.025500 |
| contention | 2 | morrow-stealing-r1 | 246.216 | 233.406 | 1101 | 256 | 0.035958 | 0.076917 |
| contention | 2 | elixir-beam | 143.647 | 10.989 | 23386 | 256 | 0.040125 | 0.161458 |
| contention | 2 | morrow-stealing-r32-experimental | 232.633 | 209.056 | 1229 | 256 | 0.031500 | 0.048292 |
| lifecycle | 2 | morrow-pinned-r1 | 19.036 | 7.430 | 103361 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r1 | 41.551 | 28.796 | 26670 | 0 |  |  |
| lifecycle | 2 | elixir-beam | 136.434 | 3.599 | 213407 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r32-experimental | 37.589 | 28.612 | 26842 | 0 |  |  |
| request-reply | 4 | morrow-pinned-r1 | 74.437 | 61.477 | 260258 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r1 | 124.800 | 100.322 | 159487 | 0 |  |  |
| request-reply | 4 | elixir-beam | 147.410 | 10.444 | 1532017 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r32-experimental | 116.206 | 93.404 | 171298 | 0 |  |  |
| contention | 4 | morrow-pinned-r1 | 240.597 | 216.484 | 1187 | 256 | 0.015042 | 0.021417 |
| contention | 4 | morrow-stealing-r1 | 239.032 | 216.817 | 1185 | 256 | 0.038292 | 0.071209 |
| contention | 4 | elixir-beam | 147.894 | 11.065 | 23227 | 256 | 0.040083 | 0.094417 |
| contention | 4 | morrow-stealing-r32-experimental | 233.411 | 206.546 | 1244 | 256 | 0.029416 | 0.103750 |
| lifecycle | 4 | morrow-pinned-r1 | 19.032 | 7.792 | 98568 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r1 | 37.828 | 26.470 | 29014 | 0 |  |  |
| lifecycle | 4 | elixir-beam | 137.049 | 3.766 | 203919 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r32-experimental | 37.739 | 25.321 | 30331 | 0 |  |  |
