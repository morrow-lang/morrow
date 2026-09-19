# Stage 3 compiler review and accepted contract

2026-09-19, read-only review of `../superpowers/specs/2026-09-19-typed-otp-process-model.md` against stage1 commit73afafc plus the in-progress stage2 compiler. No stage3 implementation or acceptance claimed.

## Frozen schemas

- Supervisor.Error tags0..12: InvalidOptions, ResourceLimit, ForeignInvocation, UnsupportedContext, WrongChildKey, DuplicateName, AlreadyRunning, AlreadyPresent, Restarting, Stopped, Removed, StartFailed(String, Process.ExitReason), SupervisorStopped. Boxed ordinary canonical enum, including nullary variants.
- ChildState: tag0 Running(ProcessId), tag1 Stopped, tag2 Restarting.
- ChildKind: tag0 Worker, tag1 Branch.
- ChildInfo ordinary record fields in order: name:String, kind:ChildKind, state:ChildState.
- Descriptor kinds15 Supervisor.Handle,16 Supervisor.ChildKey(M),17 Supervisor.ChildSpec are unallocated in reviewed actor descriptor namespace. ChildSpec runtime copy/cost/GC must traverse the retained template graph; zero descriptor children never means zero internal roots.
- Genuine generic opaque ChildKey(M) compiler representation is accepted. Recommended minimal clear extension: `Type::ChildKey(Box<Type>)`; current `NativeType` is Copy and non-generic. Preserve M through unification, substitution, aliases, public IR validation and current(handle,key) result Pid(M). No public constructor or field access.
- Checked fault15 is reserved for terminal init_ignore/init_fail outside valid startup. It is not infrastructure11 or checked9. Search found no existing checked15 assignment; add native/host diagnostic mapping with implementation.

## Required effect specialization

Parent explicitly accepted root/actor specialization for helpers reachable in both contexts. Supervisor.start/current/stop cannot be classified like synchronous Process.spawn. The current checker propagates actor mailbox effects transitively via `check/actors.rs::attach`; ordinary helper functions have mailbox=None and currently use normal synchronous calls even when called by actors. Merely emitting root_request whenever the containing function has no mailbox would therefore block an actor callback through an ordinary helper.

Introduce a contextual suspension effect and specialize affected source helpers (including transitive helper chains and closure targets) into root-driving and actor-CPS instances. Source declarations remain shared; specialized instances carry explicit effect/context identity. Root entry accepts only start/current/stop; start_link and actor startup operations reject root. Parent rejected a narrower root direct-call restriction. Ordinary callback types must not silently acquire actor context. The specialization key must distinguish root from each actor mailbox even when ordinary parameter/result types coincide.

## Private ABI and ownership

Retain existing Exec/Type/Function layout and callback ABI. Actor request is a real private SuspendSupervisor operation; root uses a separate bounded driving adapter. Neither a plain extern call that polls recursively nor a root adapter entered from an actor is permitted.

Immutable registration binds opcode + exact canonical request descriptor + exact result descriptor + generated resume Function identity. Resume frames are ordinary immutable captured frames, never reusable self-tail scratch. Public typed IR cannot mint private request/take_reply Plan nodes or substitute a shape-compatible descriptor for canonical authority.

Request admission validates and roots request/frame, charges transient replacement and reserves reply capacity before publication. Failure preserves old frame/state. Immediate typed error still becomes one resume result. On success physical callback ends with supported scheduler status and no ScopeLeave: existing mandatory cleanup stack survives suspension.

Reply adoption checks request serial, caller generation and supervisor identity; copies/reconstructs into the caller heap; stores in scanned Actor-owned storage; publishes one resume. take_reply is owner-only and once-only, from the authorized callback. Generated code installs its returned value in an active root slot before the Actor-owned root is cleared or any collecting call can occur. Runtime/compiler must agree the exact handoff; no unrooted interval is allowed. Then normal source Result matching resumes. Late replies only release their tickets.

Root request rejects actor Exec, drives bounded invocation turns, preserves first fault/cancellation, and rolls startup back before releasing root storage. Root adapter does not waive Result duties.

init_ack creates a Result(Unit,Process.Error) obligation. init_ignore/init_fail reuse stage2 internal Never and terminal-summary semantics, preserving earlier Result duties and mandatory cleanup rather than inventing normal continuation. Their operands remain left-to-right; faulting reason evaluation never commits terminal intent.

## Descriptor choice frozen after concrete path review

Parent froze kind16 count1/children[0]=mailbox using the unchanged Type layout, matching Pid kind6. The child is a type constraint, never a traced payload slot. Descriptor validation traverses the child descriptor; value cost/copy validates the key token and mailbox then handles only its opaque wrapper/control ownership. GC follows actual initialized wrapper words, never descriptor count. Current compares token AND mailbox, never name alone. Concrete required paths and test oracles are recorded in `supervisor-context-map.md`.

The same report maps actual context-specialization machinery and draft sources in `/tmp/morrow-stage3-context-fixtures-20260919/`. Parent subsequently accepted sealed internal RootFunction (not source-namable, no implicit ordinary-callback or actor-capture conversion), contextual direct helpers/fresh actor entry instantiation, and take_reply(exec,registration,out_root_slot)->status with the output slot registered before the call. Concrete remaining registration proposal and all entry-path integration are in `supervisor-registration-abi.md`. No stage3 source implementation or builds have occurred.

## Required compiler/native acceptance

Red-first: same helper and transitive helper called from root and actor produce distinct adapter/CPS bodies; blocked actor RPC permits sibling progress; root cancellation rollback; helper closure context cannot escape into ordinary callback. Forge opcode/result/resume mismatch in native/public IR and reject before actor mutation. Collect during request admission, result adoption and immediately before resume matching, including String/PID/ChildSpec captures and migrated caller. Preserve prior Result obligations across startup terminal helpers and suspended operations. Suspend must not execute defer; completion or terminal cleanup must execute it once. Source/native independent output matrix S1/S2/S4 with stealing off/on follows stage2 practice.

Implementation awaits stage2 gate and parent GO. Compiler owns crates/morrow, runtime agent owns crates/morrow-runtime; root owns docs and acceptance publication.
