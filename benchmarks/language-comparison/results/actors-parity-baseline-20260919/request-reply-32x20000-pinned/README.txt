Request/reply baseline sampling, 2026-09-19 (directory suffix is a uniqueness token).
Binary: /tmp/morrow-actors-release-20260919-transfer-gc-final
SHA256: 9de8abc625272d941a9e42aaa7ad7a25d09fe48e08012d8c78ed66327bcd0556
Each run: MORROW_SCHEDULERS={1,2,4} MORROW_REDUCTIONS=1 MORROW_WORK_STEALING=0 binary request-reply 32 20000
One-second sample at 1ms interval per run, process timeout10s. Exact stdout verified; empty stderr and exit0.
Independent oracle: replies=C*R; checksum=3*100000*R*C*(C-1)/2 + 3*C*R*(R-1)/2 + C*R = 2995199680000.
Expected stdout: ready\nrequest-reply,32,20000,640000,2995199680000\n
results.json times INCLUDE sampling overhead and are not acceptance benchmark times.
cumulative.json counts inclusive samples, suppressing recursion; categories overlap.
Single scheduler largest broad category: allocator and GC, with substantial BTree metadata maintenance.
2/4 scheduler mutex waits+kernel release totals214/431; set_idle accounts95/194, transport drain45/91.
Proposed first experiment: avoid redundant activity lock for sole-writer idle state already unchanged, add unlocked negative quiescence checks retaining locked final recheck. No source edits/builds performed for this diagnosis.
