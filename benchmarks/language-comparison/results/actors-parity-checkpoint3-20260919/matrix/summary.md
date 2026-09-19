| Workload | Schedulers | Implementation | Median process wall ms | Median ready-to-completion ms | Median operations/s | Minimum load-covered probe intervals | Median run p99 observed completion gap ms | Maximum run p99 observed completion gap ms |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| request-reply | 1 | morrow-pinned-r1 | 62.108 | 49.486 | 323323 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r1 | 61.852 | 48.206 | 331911 | 0 |  |  |
| request-reply | 1 | elixir-beam | 151.129 | 14.827 | 1079082 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r32-experimental | 63.718 | 50.029 | 319815 | 0 |  |  |
| contention | 1 | morrow-pinned-r1 | 42.965 | 29.609 | 8680 | 256 | 0.007292 | 0.013541 |
| contention | 1 | morrow-stealing-r1 | 43.179 | 29.978 | 8573 | 256 | 0.009791 | 0.019833 |
| contention | 1 | elixir-beam | 149.008 | 15.950 | 16113 | 52 | 0.275875 | 0.749375 |
| contention | 1 | morrow-stealing-r32-experimental | 37.718 | 27.351 | 9396 | 256 | 0.015709 | 0.033583 |
| lifecycle | 1 | morrow-pinned-r1 | 18.939 | 8.873 | 86550 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r1 | 18.735 | 8.854 | 86740 | 0 |  |  |
| lifecycle | 1 | elixir-beam | 134.014 | 3.841 | 199970 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r32-experimental | 18.950 | 8.854 | 86736 | 0 |  |  |
| request-reply | 2 | morrow-pinned-r1 | 67.150 | 50.151 | 319037 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r1 | 94.075 | 82.289 | 194437 | 0 |  |  |
| request-reply | 2 | elixir-beam | 140.171 | 6.032 | 2652447 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r32-experimental | 84.664 | 62.951 | 254168 | 0 |  |  |
| contention | 2 | morrow-pinned-r1 | 49.494 | 40.142 | 6402 | 256 | 0.015000 | 0.016000 |
| contention | 2 | morrow-stealing-r1 | 49.656 | 39.838 | 6451 | 256 | 0.013125 | 0.033917 |
| contention | 2 | elixir-beam | 141.299 | 10.878 | 23626 | 256 | 0.042709 | 0.176542 |
| contention | 2 | morrow-stealing-r32-experimental | 37.682 | 25.966 | 9897 | 256 | 0.018500 | 0.020667 |
| lifecycle | 2 | morrow-pinned-r1 | 18.950 | 7.055 | 108863 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r1 | 43.467 | 31.957 | 24032 | 0 |  |  |
| lifecycle | 2 | elixir-beam | 130.186 | 3.077 | 249587 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r32-experimental | 44.048 | 32.286 | 23787 | 0 |  |  |
| request-reply | 4 | morrow-pinned-r1 | 66.969 | 53.111 | 301253 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r1 | 75.203 | 52.087 | 307181 | 0 |  |  |
| request-reply | 4 | elixir-beam | 142.807 | 10.403 | 1537969 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r32-experimental | 74.748 | 50.694 | 315621 | 0 |  |  |
| contention | 4 | morrow-pinned-r1 | 50.067 | 40.191 | 6394 | 256 | 0.012834 | 0.020458 |
| contention | 4 | morrow-stealing-r1 | 50.198 | 40.138 | 6403 | 256 | 0.020416 | 0.027541 |
| contention | 4 | elixir-beam | 147.046 | 10.959 | 23451 | 256 | 0.060875 | 0.162375 |
| contention | 4 | morrow-stealing-r32-experimental | 37.718 | 25.782 | 9968 | 256 | 0.017708 | 0.031041 |
| lifecycle | 4 | morrow-pinned-r1 | 18.116 | 7.665 | 100192 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r1 | 37.800 | 29.357 | 26161 | 0 |  |  |
| lifecycle | 4 | elixir-beam | 162.034 | 4.278 | 179525 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r32-experimental | 37.759 | 28.682 | 26777 | 0 |  |  |
