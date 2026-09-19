# Stage3 request registration ABI and entry-path integration

2026-09-19 bounded design review. No stage3 repository edits/builds. Parent has accepted sealed internal RootFunction and output-root take_reply. The signatures and publication rules below are accepted design requirements; implementation and acceptance remain pending.

## Additive C ABI

```rust
#[repr(C)]
pub struct RequestRegistration {
    pub opcode: i64,
    pub request: *const Type,
    pub result: *const Type,
    pub resume: *const Function,
}
// 32 bytes, four initialized words on supported 64-bit native targets.
morrow_supervisor_register(exec: *mut Exec,
    table: *const *const RequestRegistration, count: i64) -> i64
morrow_supervisor_request(exec: *mut Exec,
    registration: *const RequestRegistration,
    request: i64, resume_frame: *mut c_void) -> i64
morrow_supervisor_take_reply(exec: *mut Exec,
    registration: *const RequestRegistration, out_root_slot: *mut usize) -> i64
morrow_supervisor_root_request(exec: *mut Exec,
    registration: *const RequestRegistration, request: i64) -> i64
```

Register returns0 success,3 fault. Request returns existing physical scheduler status0/1/3, always ends the generated callback. take_reply returns0 success,3 fault. root_request returns its boxed Result word with existing fault-cell precedence; neutral0 only when faulted. All parameters/returns except the native pointer spellings are full-width I64 in compiler runtime_abi.rs. No Exec, Type or Function layout change. The table is a pointer array, matching the existing Function-table emission helper.

Opcode0=start,1=start_link,2=current,3=stop. Root registration has resume=null and permits only0/2/3. Actor registration has a nonnull exact pointer to a Function in this invocation's function table, step present/select absent, with canonical caller mailbox. Request validates the supplied immutable frame resolves to that exact registered Function; mere equal layout/capture count is insufficient. One generated actor callsite normally has one record because its resume identity differs; root records can deduplicate by opcode/request/result identities. Original source cannot construct private records or request/resume Plan nodes.

Runtime owns one immutable registration set per invocation. Prefer a OnceLock<Arc<RegistrationSet>> inside the existing relations::Registry: that registry already survives local→parallel attachment and is shared by all workers; parallel::configure already transfers it into Shared.processes. Avoid a TLS-only registry or per-Session copy that workers created during morrow_managed_new would miss. The set owns validated snapshots keyed by exact original record addresses, not untrusted subsequent field reads. Native callers promise record/table/Type/Function/callback immutability and lifetime through stop/close including worker joins, just like existing Function registration. Copying record fields avoids accidental later rereads but cannot make dangling descriptors safe.

Registration is a root-owner operation, outside callbacks. Publish the complete set atomically after all validation and admission; never publish a valid prefix. Zero count is a no-op (null table allowed) and does not freeze the set. Repeating an identical nonempty set succeeds without allocation/charge. Any different nonempty set after publication faults11; use one combined table for a custom host, not incremental actor-time registration. Immutable lookup after publication needs no mutation. Registration itself never polls, spawns children or blocks on callbacks. Standard compiled constructors register before source can spawn anything; an external native caller must do likewise.

## Validation, lifetime and limits

- Count0..4096, nonnull table for nonzero count; nonnull unique record pointers; bounded aggregate traversal uses existing descriptor WORK and4096-type limits. Reject unknown opcodes, null mandatory descriptors, malformed kinds/children, invalid root/actor resume mode, and missing/selector resume Function before changing invocation state. Keep foreign native pointers under the same unsafe readable-array contract as managed_open; validation is not an address-space sandbox.
- Validate exact operation schema at registration. Requests are private canonical boxed records: Start/StartLink=[Flags,List(ChildSpec)], Current(M)=[Handle,ChildKey(M)], Stop=[Handle]. They have tag0 and declared arity. Results are Result(Handle,Supervisor.Error), Result(Pid(M),Supervisor.Error), Result(Unit,Supervisor.Error), respectively. Current's key child constraint must equal the result Pid mailbox descriptor. Runtime validates full frozen Error/Flags schema and stores these accepted canonical descriptor identities. A caller cannot supply a replacement shape-compatible descriptor at request/take time because neither call accepts one.
- The registration record address is the authority key within one invocation epoch, not a global credential. An unsafe native caller may provide its own valid immutable schemas/resume callbacks. Public typed IR is validated before compiler-private Plan creation, so source does not gain that native capability.
- Store sorted compact entries `(record_address, snapshot)` for bounded lookup, rather than allowing unbounded caches. Maximum4096 records also fits the compiler function/continuation limit. Admission charges the actual fixed metadata allocation (checked entry_count * size_of<Entry> + RegistrationSet/Arc overhead) before publication; failure sets checked9/status3 and releases all temporary reservations. Maintain physical control accounting separately; no pending-request quota is consumed by static metadata. Set retention may own only Budget/epoch accounting, never Shared/Registry cycles. Descriptor memory is borrowed, not logical copied payload. Release metadata ownership at invocation teardown after joins; retained process identities may keep inert epoch state but cannot execute stale registrations.
- Repeated identical register can compare addresses and validated snapshots without allowing changed native data. Compiler metadata is static data in the object, so no root registration is required for these descriptors themselves.

## Output-root handoff

Before take_reply, validate active owner actor, running callback exactly registration.resume, matching outstanding request serial/caller generation/registration, ready reply, and once-only state. Validate out_root_slot alignment and membership in the current owner's active native root frame before writing; reject null, foreign heap/frame, past-end/overflow, wrong callback and repeated take with fault11 and no reply consumption.

`memory::Domain::frames` already records heap, pointer, words, token. Add a small owner-only membership predicate using checked range arithmetic. Prefer the current/top frame for the current actor heap (the generated resume frame is live); do not accept arbitrary root ranges, the Actor's persistent roots or another actor's frame. A Root token owned by a host is not enough. Unsafe native callers still promise writable storage; existing frame_enter accepts a const pointer for tracing and cannot prove actual page permissions. The new take_reply contract explicitly requires one writable initialized word.

Compiler roots.rs reserves a zero-initialized slot in its hoisted native root frame and exposes the slot address. Runtime writes the reply into this registered slot first, test hook collects precisely at this point, then clears Actor-owned reply storage/ticket and marks taken. No allocation or GC can occur between validating slot lifetime and writing on the same owner thread. Compiler loads the returned payload from the slot after fault/status checks. The Actor root must retain reply until this transfer; suspension carries that root and reservation through migration. No additional public ABI field is needed.

## Every current entry path

1. **Native CLI / morrow_main**: `lowering/actors.rs::actor_main` calls managed_new, checks nonnull, then explicitly roots Exec through morrow_gc_frame_enter. Insert register after this Exec root and before source main. On register3, route to common managed_stop/root-frame-leave/report-fault path without reading an uninitialized main result. That requires an initialization-failure branch, not a fallthrough into current Result-main handling. No supervisor records means no extra runtime call.
2. **Generated native library / web server**: `Emitter::actor_library` currently emits morrow_library_open as a one-call forwarder to managed_open. Expand only its open wrapper: open establishes the persistent host root; register its generated table; on error close the new invocation and return null preserving first fault. string_port remains unchanged. The web application build path is `crates/morrow-web-app/build.rs`: check_library→native_library::lower→Cranelift object/archive. `morrow-web-app/src/host.rs::Room` calls morrow_library_open before creating its string port/exports. Therefore generated constructor registration automatically covers web metadata; no handwritten duplicate table and no web host polling inside an actor.
3. **Native library export selection**: `native_library::lower` currently rejects duplicate source-name matches before checking mailbox. Context specialization creates Root and Actor(M) functions with the same source name. Select the unique Root/Plain non-capturing mailbox=None source instance for host export; reject remaining ambiguity. Never export an actor CPS copy as an ordinary host adapter. Export signatures remain `(fault, exec, source args...)`.
4. **External managed_open/new**: signatures stay unchanged. Native callers providing their own Function table call supervisor_register explicitly after opening/rooting Exec and before invoking source/callbacks. Export a generated `morrow_library_register_requests(exec)->status` convenience wrapper for a custom host using compiled metadata; it forwards the exact generated table. Its supplied Exec must already contain the compiled Function descriptors or registration faults11. The normal library constructor calls this same helper. An unregistered compiled supervisor call faults11 before mutation; existing programs without supervisor RPCs require no registration. Do not silently mutate registration from actor callbacks or every arbitrary export prologue.
5. **Explicit later parallel configuration / simulation**: a native host may open local, register, then select schedulers. Existing relations registry transfer in parallel::configure must preserve the same OnceLock set; workers resolve it through Shared.processes. Configured managed_new may create workers before register, but none can execute generated source before constructor returns; publication through the shared registry makes the set visible to all. Include both orderings in deterministic tests.
6. **Root helpers/adapters**: context specialization selects root_request only for Root instances. Registration occurs at invocation construction, not every nested helper. Root adapters receive the exact root-mode record, retain request/result roots while driving bounded existing poll turns, preserve cancellation/first fault, and reject actor Exec before any polling. start_link root record is rejected at registration and checking. No initialization path drives scheduler work.
7. **REPL, wasm/client and ordinary callbacks**: supervisor execution remains native-only, matching current Process operations. Reject at existing backend/effect boundaries; do not invent an alternate unregistered path. FFI/unsafe native entrypoints use the explicit native registration contract above.

The custom-host convenience wrapper does not solve composition of arbitrary separately compiled Function tables; existing runtime already requires one complete Function table. This proposal preserves that contract rather than adding dynamic linking.

## Independent acceptance cases

- Standard CLI and generated library/open run root helper→actor helper fixtures without explicit user registration; web generated constructor publishes metadata before its first export.
- Custom native open with valid Function/registration tables works; unregistered request faults11 without state mutation. Bad last record proves all-or-nothing rollback. Same table twice consumes no additional bytes; different table rejected.
- Register before and after parallel configuration, then execute actor RPC on another owner and migrate caller before reply; exact same record identity works.
- Correct-shaped but unregistered record, wrong opcode/mode, changed result descriptor, foreign resume descriptor, wrong frame identity, duplicate record and mismatched key mailbox all reject before publication.
- take_reply rejects unregistered/unaligned/past-end/wrong-owner root slots and preserves ready reply. GC after registered-slot store but before Actor-root clearing retains String/PID/ChildSpec graphs; clear both roots then collection reclaims them. Repeated take rejects without a second reply.
- Native root_request with actor Exec faults without recursively polling; test hook counts root calls independently of stdout. Registration never invokes callbacks. Close/cancel during delayed ack releases request/reply/template reservations and joins before descriptor lifetime ends.

Stage 2 acceptance is tracked separately. This document does not establish stage 3 implementation or validation.
