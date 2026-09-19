# Stage3 runtime ownership and ABI handshake

Worktree `/tmp/morrow-typed-supervision-20260919`, base7166d081. Shared target `/tmp/morrow-typed-process-model-target-20260919`. No old-worktree builds.

## Opaque values (heap agent)

`managed/supervisor/values.rs` owns constructors, validation and kinds15/16/17 copy/cost integration. Handle is repr(C) ProcessId identity: Actor*, generation, Epoch*. Authority additionally requires immutable ActorIdentity.supervisor_process; retired handles remain valid values. ChildKey has token:*const KeyToken and name:*mut c_char. KeyToken owns immutable Arc<Epoch>, checked fresh nonzero serial and mailbox:*const Type. It owns no Session or payload pointers. Logical graph cost includes wrapper/name/fixed token footprint, physical token allocation charged once.

ChildSpec is14 fully initialized words in order: kind:u64(worker0/branch1), key:*mut ChildKey, name:*mut c_char(branch), initializer:*mut c_void(worker), children:*mut *mut ChildSpec(branch owned flat pointer array), children_len:u64, restart:i64, shutdown_kind:i64, shutdown_ms:i64, significant:i64, strategy:i64, intensity:i64, period_seconds:i64, auto_shutdown:i64. Inactive fields zero. Actual native payload links are copied/traced; no foreign template heap is retained. Cost/copy traverse frames, captures and branch graphs in the SAME bounded traversal/memo; engine separately checks structural depth64/direct children1024/unique names even for aliased DAGs.

Existing cost::value/copy::value/value_fragment signatures stay unchanged. Engine conveniences: cost::child_spec(s,spec)->Option<usize>; copy::child_spec(s,spec)->Copy. values::valid_handle(s,handle) and valid_key(s,key,mailbox) validate epoch/token/mailbox and immutable role, not foreign mutable engine state. Runtime owner provides supervisor::is_handle_actor(a)->bool and key_identity(s)->Option<(Arc<Epoch>,u64)>.

Synchronous symbols (all compiler parameters/results I64): morrow_managed_supervisor_child_key(exec,name,mailbox)->ChildKey; ..._worker(exec,key,entry,policy)->boxed Result ChildSpec; ..._branch(exec,name,flags,children,policy)->boxed Result ChildSpec; ..._id(exec,handle)->ProcessId. Constructor inputs are borrowed/rooted and copied into current owner heap. Malformed native ABI is fault11; plain child_key bound/admission failure is checked9.

## Canonical schema and startup

Flags tag0 fields Strategy,Int intensity,Int period_seconds,AutoShutdown. Strategy tags OneForOne0,OneForAll1,RestForOne2. AutoShutdown Never0,AnySignificant1,AllSignificant2. ChildPolicy tag0 fields Restart,Shutdown,Bool significant. Restart Permanent0,Transient1,Temporary2. Shutdown Graceful(Int)0,Infinity1,Immediate2. Supervisor.Error tags0..12 follow accepted compiler contract.

Private request schemas: Start/StartLink record tag0 [Flags,List(ChildSpec)]; Current(M) tag0 [Handle,ChildKey(M)]; Stop tag0 [Handle]. Results respectively Result(Handle,Error), Result(Pid(M),Error), Result(Unit,Error). Registration opcodes0=start,1=start_link,2=current,3=stop. Record32bytes opcode/request descriptor/result descriptor/resume Function. Root resume null and no opcode1. Exact registered actor resume identity, mailbox and frame identity are mandatory.

Startup symbols: morrow_process_init_ack(exec)->boxed Result(Unit,Process.Error); init_ignore(exec)->status2; init_fail(exec,reason)->status2. Terminal invalid-startup context is checked15/status3; preexisting/operand fault wins. ack invalid/repeated context gives Process.InvalidOptions(tag4). No implicit startup timeout.

## Registration and reply ownership (runtime owner)

Use a bounded Registry registration state with one-time publication and explicit owner close: Unregistered, Published(owned sorted exact-address snapshots), Closed. Root registration validates every record/schema/resume and charges all metadata before publishing; duplicate identical publication is free/idempotent, different publication fails11, count0 is no-op. Lookup copies one immutable record under read lock then releases it before allocation, callbacks or GC. Root stop/close after worker joins takes published storage and releases logical/physical metadata even when inert PIDs retain Session/Shared/Registry. No foreign Session mutation from Drop. Registration-before-parallel and after-parallel both preserve authority.

Actor gains a seventh contiguous GC root for reply, with updated offset assertions and a precise-GC oracle. Callback identity is tracked owner-locally so take_reply requires exact registered resume currently executing (selectors/defer callbacks cannot impersonate it). take_reply validates an aligned writable word in current actor's top native root frame, writes first, runs precise-GC test hook, then clears Actor reply ownership. Root ranges and other-owner frames do not grant handoff authority.

Engine actors are isolated and initially pinned; workers remain ordinary movable isolated actors. Template graph lives in supervisor payload heap and each generation copies its initializer. Requests suspend actor continuations; root adapters alone drive bounded poll turns. Existing cleanup stack survives suspension. Generation/request serial and exact registration identify every reply; stale replies release reservation only.

Stage3 includes static OneForOne startup/ack/current/stop, Permanent/Transient/Temporary selection, inclusive rolling whole-second intensity, intensity0, failed restart attempts charged and initial startup uncharged. Fresh generation identities preserve stale PID behavior. Stage4 adds grouped strategies and nested/ordered escalation; stage5 adds dynamic/significant-child management. Existing legacy supervise is unchanged.

Opening tests: registration valid/all-or-nothing/idempotent/forged schemas and resume; preparallel registry preservation; retained PID after close metadata cleanup; seventh reply precise root; output-root membership and GC handoff. Then actual suspended request/startup acknowledgement, cancellation, source helper context and inclusive restart-window fixtures. No broad gate starts before root's upcoming quiet measurement window.
