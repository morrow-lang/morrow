| Workload | Schedulers | Implementation | Median process wall ms | Median ready-to-completion ms | Median operations/s | Minimum load-covered probe intervals | Median run p99 observed completion gap ms | Maximum run p99 observed completion gap ms |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| request-reply | 1 | morrow-pinned-r1 | 67.867 | 56.253 | 284430 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r1 | 69.033 | 56.243 | 284480 | 0 |  |  |
| request-reply | 1 | elixir-beam | 148.559 | 13.913 | 1150017 | 0 |  |  |
| request-reply | 1 | morrow-stealing-r32-experimental | 69.060 | 57.077 | 280321 | 0 |  |  |
| contention | 1 | morrow-pinned-r1 | 452.056 | 440.501 | 583 | 256 | 0.012208 | 0.014250 |
| contention | 1 | morrow-stealing-r1 | 448.955 | 438.947 | 585 | 256 | 0.012708 | 0.025500 |
| contention | 1 | elixir-beam | 148.596 | 15.909 | 16154 | 53 | 0.272417 | 0.285541 |
| contention | 1 | morrow-stealing-r32-experimental | 454.483 | 440.905 | 583 | 256 | 0.048000 | 0.064750 |
| lifecycle | 1 | morrow-pinned-r1 | 18.935 | 9.272 | 82830 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r1 | 18.951 | 9.251 | 83019 | 0 |  |  |
| lifecycle | 1 | elixir-beam | 130.660 | 3.813 | 201436 | 0 |  |  |
| lifecycle | 1 | morrow-stealing-r32-experimental | 19.015 | 9.236 | 83157 | 0 |  |  |
| request-reply | 2 | morrow-pinned-r1 | 87.498 | 74.308 | 215320 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r1 | 93.278 | 84.069 | 190320 | 0 |  |  |
| request-reply | 2 | elixir-beam | 140.088 | 6.041 | 2648404 | 0 |  |  |
| request-reply | 2 | morrow-stealing-r32-experimental | 87.844 | 75.961 | 210634 | 0 |  |  |
| contention | 2 | morrow-pinned-r1 | 493.533 | 467.494 | 550 | 256 | 0.023042 | 0.024125 |
| contention | 2 | morrow-stealing-r1 | 503.289 | 492.542 | 522 | 256 | 0.031500 | 0.081083 |
| contention | 2 | elixir-beam | 143.828 | 10.770 | 23863 | 256 | 0.027417 | 0.034125 |
| contention | 2 | morrow-stealing-r32-experimental | 464.333 | 437.697 | 587 | 256 | 0.037625 | 0.043208 |
| lifecycle | 2 | morrow-pinned-r1 | 18.294 | 7.616 | 100846 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r1 | 36.769 | 27.350 | 28080 | 0 |  |  |
| lifecycle | 2 | elixir-beam | 131.635 | 3.460 | 221968 | 0 |  |  |
| lifecycle | 2 | morrow-stealing-r32-experimental | 37.171 | 26.371 | 29122 | 0 |  |  |
| request-reply | 4 | morrow-pinned-r1 | 87.858 | 76.878 | 208123 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r1 | 112.832 | 82.905 | 192992 | 0 |  |  |
| request-reply | 4 | elixir-beam | 143.026 | 10.214 | 1566426 | 0 |  |  |
| request-reply | 4 | morrow-stealing-r32-experimental | 96.808 | 73.525 | 217612 | 0 |  |  |
| contention | 4 | morrow-pinned-r1 | 504.097 | 470.038 | 547 | 256 | 0.021666 | 0.024750 |
| contention | 4 | morrow-stealing-r1 | 485.805 | 469.452 | 547 | 256 | 0.028000 | 0.038084 |
| contention | 4 | elixir-beam | 144.232 | 10.912 | 23551 | 256 | 0.027584 | 0.096583 |
| contention | 4 | morrow-stealing-r32-experimental | 464.891 | 441.367 | 582 | 256 | 0.038584 | 0.070791 |
| lifecycle | 4 | morrow-pinned-r1 | 18.186 | 7.752 | 99072 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r1 | 37.717 | 26.289 | 29214 | 0 |  |  |
| lifecycle | 4 | elixir-beam | 142.749 | 4.284 | 179287 | 0 |  |  |
| lifecycle | 4 | morrow-stealing-r32-experimental | 37.771 | 25.829 | 29734 | 0 |  |  |
