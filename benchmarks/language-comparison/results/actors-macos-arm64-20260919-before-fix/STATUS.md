# Aborted pre-fix actor matrix

This is failure evidence, not a completed language comparison. The release
runner completed 36 semantic verification processes and recorded 97 successful
timed samples before the 98th timed attempt exceeded the unchanged 180-second
bound.

The failing cell was `morrow-stealing-r1`, two schedulers, contention warmup:

```text
MORROW_SCHEDULERS=2
MORROW_REDUCTIONS=1
MORROW_WORK_STEALING=1
contention 2 257 2000000
```

Its stdout reached `sample,78` without `hot-done` or the verified summary; stderr
was empty. The same pinned workload completed in about 465 ms. The runner killed
the owned process group and retained the partial streams and reason under
`failures/`. No process was left behind.

Sampling and source inspection identified root-driver throttling rather than a
deadlock: the parallel driver waited 10 ms after every busy 64-callback poll even
when its root queue remained runnable. Stealing the two-million-step hot actor
onto that queue therefore introduced roughly 31,250 waits. The ordered markers
through `sample,78` are consistent with slow periodic progress. The focused stack
evidence is retained in `diagnosis.txt`.

The matrix was stopped and renamed before any summary was generated. No sample
was retried, trimmed or reclassified. A fresh dated directory must be used after
the runtime fix and a new quiet signal.
