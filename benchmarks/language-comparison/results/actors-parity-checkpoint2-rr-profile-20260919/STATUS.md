# Checkpoint 2 request/reply profile

This is a bounded diagnostic profile, not an acceptance timing run. It samples
the immutable GC/private-frame workload artifact
`56e72093114fbe17655074093f1d91b46e208f251da9821264e57534d1d2e419`
with `request-reply 32 20000`, two schedulers, reductions 1 and stealing enabled.
The process and sampler both exit zero. The exact output is 640,000 replies and
checksum 2,995,199,680,000, with empty workload stderr. Inputs and raw streams are
retained. Unrelated process-model correctness builds could run during this
profile; its elapsed time is not compared with quiet benchmarks.

The two scheduler threads contribute 2,916 samples, including 345 in condition
variable waits. Across the 2,571 remaining samples, inclusive heap allocation
contains 607 (23.6%), fragment adoption 164 (6.4%) and heap detachment 55 (2.1%).
`sample-counts.json` records these sums over the raw call graph. These categories
describe observed stacks, not independent additive percentages or a causal
speedup prediction. The top exclusive frames include allocation, tree insertion,
memory movement and thread-local domain access. Detachment is no longer the
dominant stack seen in the preserved checkpoint1 regression profile.

Inspection of the pinned nightly standard-library source found a specific
fragment-adoption cost to test: `BTreeMap::append` uses an ordered merge cursor
that linearly advances through target keys between successive source keys.
A tiny message fragment with widely separated allocation addresses can therefore
scan unrelated receiver allocations. A comparator-count regression and actual
ownership/accounting tests can distinguish sparse insertion from that merge;
the profile alone does not establish an improvement. Descriptor lookup caching
is a separate pending experiment. The unchanged BEAM matrix still passes zero
of nine strict parity cells.
