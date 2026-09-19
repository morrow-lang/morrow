# Actor parity baseline profiles

These are diagnostic stack samples for the actor performance-parity work at
commit `11e6ad5b8bb6058dd61e22d25ef94440b5c5a2b2`. Both use the release workload
artifact with SHA-256
`9de8abc625272d941a9e42aaa7ad7a25d09fe48e08012d8c78ed66327bcd0556`.
They are not quiet acceptance benchmarks, and their wall times include sampling
and process-observation overhead.

## `contention-32m`

Command environment and workload:

```text
MORROW_SCHEDULERS=1
MORROW_WORK_STEALING=0
MORROW_REDUCTIONS=1
/tmp/morrow-actors-release-20260919-transfer-gc-final contention 1 257 32000000
```

The child exited zero with empty stderr, ordered markers `sample,0` through
`sample,256`, and independently verified summary
`contention,257,32000000,66049,536197235`. `/usr/bin/sample` observed the child
for two seconds at 1 ms intervals. `result.json` reports the full profiled process
observation, not the `ready`-to-summary interval used by the benchmark runner.

The stack sample concentrates active time in actor callback continuation and
receive-frame replacement/costing. It is input to optimization work, not proof
that one stack category alone explains the full BEAM gap.

## `request-reply-32x20000-pinned`

This directory covers scheduler counts 1, 2 and 4 with
`MORROW_REDUCTIONS=1`, stealing disabled, and workload
`request-reply 32 20000`. Each child had a ten-second bound and a one-second,
1 ms stack sample. All three exact outputs passed the independent 640,000-reply
and checksum oracle; stderr was empty.

The one-scheduler sample is dominated by allocator, collection and tree-metadata
work. The two- and four-scheduler samples additionally show global activity-lock
contention in redundant idle publication and empty transport drains. The nested
`README.txt`, `results.json` and `cumulative.json` contain the exact commands,
counts and category definitions.

## `lifecycle-1000-500`

This directory profiles `lifecycle 1000 500` with reductions 1 for pinned
two-scheduler Morrow and stealing Morrow at two and four schedulers. Each child
exited successfully with the independently checked summary
`lifecycle,1000,500,1000,1000,8492500,249500` and empty stderr. The runs are
diagnostic and include sampler overhead.

The pinned child finished before useful stacks were captured. In the stealing
runs, active root-thread samples concentrate in `detach_heap` and
`Domain::owns`, while workers wait or contend on the shared activity lock.
`DIAGNOSIS.txt` records exact counts and the source interpretation that motivated
the bounded foreign-allocation interval index. Final performance must be taken
from a quiet matrix, not these profiles.

`files.sha256` hashes every retained input and output beneath this directory
except itself. Historical actor reports and raw result directories were not
modified to add these profiles.
