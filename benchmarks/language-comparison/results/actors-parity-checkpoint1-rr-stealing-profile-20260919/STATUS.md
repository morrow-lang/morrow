# Checkpoint 1 request/reply stealing profile

Status: complete bounded diagnostic. This profile localizes checkpoint 1's
two-scheduler stealing regression; it is not an acceptance benchmark.

The immutable checkpoint workload artifact has SHA-256
`7668ab2fc66a13c51bde232fd3b56a13b161749291d896441c7f7dead77d693c`.
Its source and toolchain provenance are recorded by the checkpoint matrix. The
profile ran `request-reply 32 20000` with two schedulers, reductions 1 and work
stealing enabled. `/usr/bin/sample` requested two seconds at one-millisecond
intervals and captured 1,415 ticks on each of the two scheduler threads before
the workload exited. The sampler and process both exited 0. Standard error was
empty. Standard output exactly matched the independent oracle: 640,000 replies
and checksum 2,995,199,680,000.

Across both scheduler threads, `transport::drain -> transfer::detach_heap`
contains 830 of 2,830 total samples (29.3%). Condition-variable waiting accounts
for 1,189 samples; relative to the remaining 1,641 samples, heap detachment is
50.6%. The large inlined `detach_heap` closure is the top exclusive frame for
596 samples (36.3% of samples outside condition-variable waits). The collapsed
exclusive stacks also contain 225 `memmove` samples, 35 `Domain::owns` samples
and 22 `BTreeMap` drop samples. Collection contains 87 inclusive samples across
the two threads, much less than detachment.

This profile strongly localizes the regression to heap detachment and its
inlined validation/index work. It is consistent with the hypothesis that an
eager foreign-allocation interval index costs more than direct live-word checks
after collection leaves a small graph. Optimized symbols do not identify every
instruction in the large closure, so the profile alone does not prove which
index operation is responsible. A focused tiny-live-graph/many-foreign-blocks
oracle and before/after measurement are needed for that causal claim.

`raw/sample.txt` is the complete sample report. The other raw files preserve
the process streams, sampler streams and exit statuses. `inputs.json` records
the command, environment, artifact hash, host and independent oracle.
