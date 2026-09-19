# Actor continuations

Each of Morrow's native schedulers runs FIFO callbacks. Native invocations can
opt into multiple scheduler threads with `MORROW_SCHEDULERS` and optional
work stealing with `MORROW_WORK_STEALING=1`; see the
[actor runtime](ACTOR_RUNTIME.md#typed-native-execution). The compiler emits separate actor
copies of eligible functions, keeping their ordinary native ABI and synchronous
behavior when called outside an actor. This is cooperative source-level
suspension, not instruction-level or operating-system preemption.

## Scheduling boundaries

Direct and dynamically invoked ordinary helpers on recursive, iterative or
oversized finite paths use managed continuation frames. This includes calls that return a
value, nested calls in strict operands, mutually recursive helpers and Unit tail
calls. `let ... else` destructuring runs after its suspended initializer and
preserves its failure branch. An operand is evaluated once, in source order. Boolean `and`/`or` operands
remain lazy. A helper result is packed into a typed caller frame; the next source
operation executes in a later scheduler callback. Pure frame factories use the
ordinary closure ABI and do not retain a native caller stack.

`for` loops over Lists, insertion-ordered Maps and first-class Ranges retain an
immutable collection and an induction value. A callback performs at most one
iteration; advancing and completion may use additional callbacks. Nested
`break`, `continue`, function `return`, and receiving inside the loop retain
their lexical meaning. Map entries become structural tuples; scalar payloads
retain their full width. Inclusive `Int` maximum endpoints are checked before
increment, avoiding overflow. Empty and descending ranges complete without
executing the body.

A suspended callback retains only its typed lexical values. Execution contexts
and fault pointers are never captured in a language frame. The existing runtime
owns actor heap roots and accounts for continuation graphs before publication.
Non-tail recursion consumes bounded managed frames and can reach a checked
resource fault; tail recursion reuses its return continuation. Supervision sees
that fault through the normal failure path.

`with` and `?` become typed Result branches before continuation conversion.
Receiving helpers can return owned values, including Results, and resume their
caller at a non-tail expression. Mailbox inference follows direct call cycles.
A propagated error exits its original logical function and runs its deferred
cleanup; matching handlers retain their outer lexical scope.

`List.map`, `filter`, `fold`, `find`, `any` and `all` publish per-element frames.
Callbacks may suspend through recursive helpers or returned/captured ordinary
closures. `find`, `any` and `all` stop before invoking later callbacks, and empty
lists never invoke a callback. Native map/filter results use an unpublished
linear list builder. `Option.map` and Result `map`, `and_then`, `unwrap_or_else`
evaluate both arguments eagerly and invoke only the selected callback.

Dynamic dispatch checks the original closure identity before reading its typed
captures. Separate resumable copies leave ordinary CLI calls and callback ABIs
unchanged. Actor-bearing programs prepare eligible recursive identities even
when a callable reaches the actor through aliases or a function return.

Straight-line prefixes count source computations, including the transitive cost
of small finite helpers. Prefixes exceeding 32 computations split into typed
continuations; oversized strict expressions normalize in source order before
splitting. This bounds eligible source work, not native instruction counts.
Small finite helpers preserve their ordinary scheduling behavior. Existing
source-depth, normalization-node and generated-function limits still apply.

`MORROW_REDUCTIONS=1..65536` controls callbacks per actor turn (default 1).
The scheduler invokes them in a loop with a fresh native callback stack each
time, then rotates the actor to the FIFO tail. An ordinary receive ends the turn
even if it selects an already queued message. Larger quanta amortize scheduling
cost at the expense of longer intervals before sibling work and timer promotion.

## Boundaries that remain synchronous

A callback cannot preempt a blocking foreign or runtime call. Such calls are a
trusted native boundary and need an asynchronous adapter for responsive actor
service under blocking workloads. First-class helpers with actor effects
(spawn/send/receive) still require the checked direct-call contracts; the dynamic
callback extension applies to ordinary functions. Match guards must remain finite
and cannot contain recursive/iterative continuation work. Receive guards retain
the stricter pure, non-failing contract. Neither guard is independently scheduled.

`defer` uses actor-owned logical scopes through suspension, return, failure and
cancellation; see [Actor-owned deferred cleanup](ACTOR_CLEANUP.md). Cleanup
callbacks run synchronously and cannot perform deferred actor effects. Helpers
with unsupported capture/result representations,
including foreign pointers and native handles, do not acquire resumable copies.
`List.sort_by` comparators remain native synchronous callbacks. Collection and
message-copying work is not charged to the callback quantum.
Native descriptor preflight also refuses foreign pointers retained in a dynamically
formed continuation graph. Message sendability is unchanged.

The compiler bounds normalization work, source depth, generated locals and
continuation identities. The scheduler bounds published frame memory and graph
traversal. These are checked limits, not an assertion that arbitrary source work
has a constant execution cost.

## Acceptance

`crates/morrow/tests/cranelift_backend.rs` contains native tests with independent
expected output and real runtime scheduling:

- a host polls a long loop one callback at a time and observes a sibling before
  the loop completes;
- non-tail numeric recursion yields before its result and preserves ordinary
  call semantics;
- nested List/Map/Range loops, Unicode keys, full-width endpoints, lexical exits
  and receives survive precise collection at each scheduling boundary;
- 64 seeded recursive tuple-return cases compare against an independent integer
  formula while retaining Unicode strings and full-width list payloads;
- strict operand order, short-circuiting and early helper returns have exact
  output oracles;
- supervised arithmetic faults and exhausted return-frame limits use seeded
  polling budgets and bounded callback counts;
- seeded List callback campaigns compare insertion order, full-width values,
  Float values, Unicode captures, filtering, folding and short-circuiting against
  independent Rust models under precise collection;
- captured and returned closure aliases retain ordinary ABI behavior outside an
  actor while recursive callbacks yield to siblings inside an actor;
- a dynamic callback fault unwinds every logical activation and prevents later
  collection callbacks;
- oversized finite helpers, transitive finite call trees and strict expressions
  yield before their final result while preserving full-width output;
- the previous 100,000-transition tail-helper and native host polling oracles
  remain regression coverage.

The REPL scheduler reuses this validated continuation IR through
`lowering::prepare_interactive_actors`, including the source-to-actor-entry map.
The same seeded List campaign runs twice per seed in fresh REPL sessions and
asserts deterministic output and retired actor counts. `actor_contracts.rs`
retains obsolete rejection cases as positive compilation contracts and checks
all current invalid actor fixtures. See [REPL actors](REPL_ACTORS.md) for lifecycle
and virtual-time behavior.
