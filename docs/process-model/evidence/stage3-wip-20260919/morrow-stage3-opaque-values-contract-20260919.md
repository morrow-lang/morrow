# Stage 3 opaque value implementation contract

Date: 2026-09-19. Read-only preparation against stage2 commit
7166d08172ce606ffc47bbc30ebfcfe5e0a4ad7e in
`/tmp/morrow-typed-supervision-20260919`. No implementation or test acceptance claimed.

## Ownership split

Heap agent: `crates/morrow-runtime/src/managed/supervisor/values.rs`, opaque-value
cases in `managed/cost.rs` and `managed/copy.rs`, and dedicated value graph tests.
Runtime engine agent: supervisor module declaration, immutable ActorIdentity role,
key serial issuance, registration, engine, requests/replies, lifecycle/relations.
Compiler agent: `crates/morrow`. Parent: final contract, docs, integration gates.
No memory collector or public Exec/Type/Function ABI changes are needed.

## Native representations

Kinds 15 Handle and 17 ChildSpec have descriptor count zero. Kind 16 ChildKey(M)
has count one, children[0] the exact mailbox descriptor. This child is a type
constraint; it is not a wrapper payload slot. Descriptor validation traverses it.

All payload records are repr(C), fully initialized, with full-word discriminants.
There are no bool/u8 fields or uninitialized scanned padding.

```
Handle { identity: process::Identity }
// Identity = actor pointer, generation:u64, epoch pointer; 24 bytes.

ChildKey { token: *mut KeyToken, name: *mut c_char } // 16 bytes
// KeyToken is inert immutable retained control containing Arc<Epoch>,
// nonzero checked serial:u64, and mailbox:*const Type. No payload pointers,
// Session, Shared, registry back-reference, callback, or owner-local Drop.

ChildSpec {                         // 14 words, 112 bytes
    kind: u64,                      // worker=0; branch=1
    key: *mut ChildKey,             // worker only
    name: *mut c_char,              // branch only; worker uses key.name
    initializer: *mut c_void,       // worker registered entry frame
    children: *mut *mut ChildSpec,  // branch owned GC-scanned pointer array
    children_len: u64,
    restart: i64,
    shutdown_kind: i64,
    shutdown_ms: i64,
    significant: i64,              // exactly 0 or 1
    strategy: i64,
    intensity: i64,
    period_seconds: i64,
    auto_shutdown: i64,
}
```

Inactive fields are zero. Empty child arrays use null with length zero. Branch
constructor accepts the ordinary List(ChildSpec) ABI but normalizes it into an
owned flat array. Names contain at most 4096 UTF-8 bytes. Wrappers and arrays are
ordinary owner-heap GC allocations, never external control records hiding payload
pointers. Native scalar words may conservatively retain unrelated storage in the
existing collector; they never justify ignoring pointer-bearing fields.

Handle construction retains Actor+Epoch exactly like ProcessId. Validation calls
process::valid_identity and `supervisor::is_handle_actor(actor)`. The latter reads
only immutable ActorIdentity.supervisor_process, initialized before publication.
It never reads another owner's mutable Engine. Dead handles remain valid values;
management operations report SupervisorStopped. Ordinary/legacy actors cannot be
reinterpreted as supervisor handles.

Each copied key wrapper retains the same token. Exact token identity and mailbox
authority are required for current; name equality is insufficient. A removed and
replaced same-name child cannot revive an old key. Token serial issuance is
`unsafe supervisor::key_identity(s:*mut Session)
 -> Option<(Arc<relations::Epoch>,u64)>`, checked nonzero/nonwrapping per invocation.

## Synchronous private constructor ABI

All arguments lower to native I64 words. Pointer syntax below documents ownership.
Symbols have prefix `morrow_managed_supervisor_`.

```
child_key(exec:*mut Exec, name:*const c_char, mailbox:*const Type) -> *mut ChildKey
worker(exec:*mut Exec, key:*const ChildKey, entry:*const c_void,
       policy:*const i64) -> i64 // ordinary boxed Result(ChildSpec,Error)
branch(exec:*mut Exec, name:*const c_char, flags:*const i64,
       children:*const abi::List, policy:*const i64) -> i64 // same Result
id(exec:*mut Exec, handle:*const Handle) -> *mut process::Identity
```

Inputs are borrowed valid native values, rooted across any collecting call.
Constructors validate first, then copy names/template/child graph into the current
owner heap; returned values enter the ordinary generated root assignment. Outputs
retain no sender payload storage. Source expressions retain left-to-right order
and fault precedence. The four helpers are synchronous and never poll a scheduler.
No equality export is introduced; it is absent from the accepted public API.

Canonical Flags record is tag0 followed by Strategy, Int intensity, Int
period_seconds, AutoShutdown. ChildPolicy is tag0 followed by Restart, Shutdown,
Bool significant. Enum tags follow declaration order in the spec. Shutdown has
Graceful=0 plus its Int payload, Infinity=1, Immediate=2. Nullary enum values are
ordinary boxed canonical values, not scalar tags passed in place of pointers.

child_key name/serial/resource failure raises checked9 because the source API has
no Result. Malformed descriptor/native schema raises infrastructure11. worker and
branch ordinary option failures produce Error.InvalidOptions (tag0), admission
failure Error.ResourceLimit (tag1), and foreign epoch Error.ForeignInvocation
(tag2). First existing checked fault is never overwritten. Canonical boxed Result
and Error layouts and normal Result duties are unchanged.

## Cost, copy, collection and transfer

`cost::descriptor` extends its kind ceiling through17 and validates kind16's
one-child rule. Cost and Copy each dispatch kinds15,16,17. Nested initializer
frames and branch child graphs use the SAME traversal instance, work counter,
byte limit, depth limit, seen set and copy memo as the containing graph. Calling
fresh public cost::value/frame recursively would silently reset budgets and is
forbidden. Initializers use registered Function validation and exact entry/mailbox
agreement, preserving descriptor lifetime and zero-argument entry requirements.

Existing cost/copy public signatures remain unchanged. Add owner-internal helpers:

```
unsafe cost::child_spec(s:*mut Session, spec:*const values::ChildSpec)->Option<usize>
unsafe copy::child_spec(s:*mut Session, spec:*const values::ChildSpec)->copy::Copy
unsafe values::valid_handle(s:*mut Session, handle:*const Handle)->bool
unsafe values::valid_key(s:*mut Session, key:*const ChildKey,
                        mailbox:*const Type)->bool
```

Convenience helpers start one bounded operation; recursion never reenters them.
Engine registered request descriptors can instead use existing cost::value,
copy::value and copy::value_fragment normally. ChildSpec header/options validation
is shared with constructors. Any helper-only descriptors stay synchronous and
cannot become invocation registration authority.

Cost includes wrapper, copied UTF-8 text including terminator, branch pointer
array, full initializer/capture graph, and conservative fixed token footprint per
key graph occurrence after ordinary alias deduplication. Physical external token
accounting is actual once per allocation; copying retains rather than recreates
the token. This does not consume relationship/control slots. Conservative logical
copies may charge shared token storage again; parent accepted this policy.
Failed admission restores the same reservation it acquired, without owner-local
foreign-thread destruction. No strong Session/Shared/registry cycle is introduced.

Copy allocates and roots/memoizes destination records before copying children.
Root slots cover partially constructed records and all intermediate boxed outputs.
Fragment mode owns every destination block and control token until adoption;
JSON memo Rc owners are still discarded before cross-thread transfer. Abort drops
partial copies/tokens exactly once. Copy preserves full-width scalars and semantic
sharing; immutable function/type descriptor pointers stay invocation-owned.

The supervisor engine copies admitted Specs into its own payload heap, retains
that graph in an explicitly scanned root, and copies each worker initializer into
each new generation. A live worker frame is never reused as the restart template.
Controllers may be pinned initially; values and ordinary workers remain movable.
Control tokens never contain foreign payload roots. Existing whole-heap transfer
validation and inert destructor rules remain unchanged.

## Separate structural tree admission

The structural contract is depth64, at most1024 direct children, unique names per
supervisor, valid policy/strategy combinations, plus actor/control/byte quotas.
Cost's generic depth128 and alias memo are not a substitute. In particular, a
shared subtree first visited shallow and then reused deeper must not evade depth64
because a cost memo returns early. Engine/constructor structural validation must
check per-path depth (or memoize subtree height) under bounded work and reject
cycles, before any child runs. ChildSpec count0 must never mean cost0 or roots0.

## Independent red-first acceptance slice

1. Descriptor15/17 count0 and16 count1 pass; wrong arities and malformed child
   descriptor fail; registered frame with kind16 capture retains its mailbox.
2. A worker template captures wide Int/Float, String, JSON, PID/ProcessId and a
   shared nested graph. Destroy sender heap, collect destination, and invoke the
   copied entry with exact independent expected values; a second generation must
   start from original captures after the first mutates its own worker state.
3. A branch template retains names and worker entries after source collection;
   copied wrappers/arrays/frames have destination addresses while token identity
   remains stable. An actual cross-OS-thread Fragment adoption has no source-owned
   payload edge, preserves JSON exclusivity, and releases control once.
4. Stale same-name keys, different mailbox, foreign invocation, dead supervisor
   handle and ordinary actor-as-handle each produce their specified independent
   result/failure, never name-based PID reinterpretation.
5. Force precise collection between constructor allocation/copy/result wrapping;
   partial allocation failure leaves reservation and physical-control counts
   balanced. Check near byte limits before and after rollback.
6. Exact native malformed shape, oversized name/options, WORK/BYTES/depth bounds,
   nested shared subtree depth64/65, and alias/cycle cases exercise real limits.
7. Layout byte-poison regression verifies every GC-scanned word is initialized;
   external crate Exec declaration still passes strict improper_ctypes/Clippy.

Tests/builds have not been run for Stage3 by this agent. Implementation awaits
parent's final handshake. No benchmark benefit or complete supervisor behavior is
claimed by this decomposition.
