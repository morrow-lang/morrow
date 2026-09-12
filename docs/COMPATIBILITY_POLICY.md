# Compatibility and Deprecation Policy

This policy defines how Fern evolves while keeping upgrades predictable.
It is enforced in CI and referenced by the release workflow.

## Compatibility Guarantees

Fern compatibility is defined for four surfaces:

1. Language syntax accepted by `fern parse/check/build`.
2. Type-checking behavior for valid programs.
3. Standard library module APIs shipped with the compiler.
4. CLI command and flag contract for stable commands.

Compatibility baseline:

1. Patch releases (`x.y.Z`) must remain backward compatible.
2. Minor releases (`x.Y.z`) may add features but must not break existing valid code.
3. Breaking changes are allowed only in major releases and must include migration notes.
4. Experimental features must be clearly marked experimental and may change before stabilization.

## Standard Library API Contract (Gate C)

As of 2026-02-06, Fern reserves and stabilizes the following top-level stdlib entry points:

1. `fs`
2. `json`
3. `http`
4. `sql`
5. `actors`

Compatibility alias policy:

1. `File.*` remains supported as a compatibility alias for `fs.*`.
2. Any future alias deprecation must follow the lifecycle below and keep the minimum 2-minor-release support window.
3. New APIs may be added to these modules in minor releases, but existing signatures must remain backward compatible.

Canonical naming policy for docs/new code:

1. Prefer `fs.*` for filesystem APIs (`File.*` is compatibility-only).
2. Keep service module names lowercase: `json`, `http`, `sql`, `actors`.
3. Keep core utility modules PascalCase: `String`, `List`, `System`, `Regex`, `Result`, `Option`, `Tui.*`.

Detailed function-level signatures are tracked in:
- `docs/STDLIB_API_REFERENCE.md`

### Gate C Runtime Contract (2026-02-06)

Gate C runtime behavior is stabilized as:

1. `json.parse(text)`:
   - `Err(FERN_ERR_IO)` for empty input.
   - `Ok(copy_of_text)` for non-empty input.
2. `json.stringify(text)`:
   - `Ok(copy_of_text)` for all string inputs (including empty).
3. `http.get(url)` / `http.post(url, body)`:
   - `Ok(response_body)` for successful (`2xx`) HTTP responses.
   - `Err(FERN_ERR_IO)` for invalid URLs, non-`2xx` responses, and transport/read failures.
   - Current runtime build uses civetweb with TLS enabled for both `http://` and `https://` URLs.
4. `sql.open(path)` / `sql.execute(handle, query)` / `sql.close(handle)`:
   - `sql.open(path)` returns `Ok(handle)` for valid paths and `Err(FERN_ERR_IO)` for invalid input/open failure.
   - `sql.execute(handle, query)` returns `Ok(rows_affected)` for valid handles/statements and `Err(FERN_ERR_IO)` for invalid handles/input/SQL errors.
   - `sql.close(handle)` returns `Ok(0)` after closure and `Err(FERN_ERR_IO)` for invalid/already-closed handles; SQLite close failures leave the handle live.
   - At most 256 native connections may be live. The next open returns `Err(FERN_ERR_OUT_OF_MEMORY)` before filesystem effects; closure releases capacity, and handle IDs are never reused.
   - SQLite behavior is regression-tested in `tests/test_runtime_surface.c` and the debug/release/sanitized [lifecycle gate](../scripts/test_runtime_sql.py).
5. `actors.start(name)`:
   - Returns deterministic, process-local, monotonic actor ids.
6. `actors.post(actor_id, msg)`:
   - Enqueues a message copy into actor mailbox FIFO and returns `Ok(0)`.
   - Returns `Err(FERN_ERR_IO)` for invalid actor ids or invalid message pointers.
7. `actors.next(actor_id)`:
   - Returns `Ok(message)` in FIFO order when mailbox is non-empty.
   - Returns `Err(FERN_ERR_IO)` for empty mailbox or invalid actor id.
8. Runtime C-ABI actor helpers:
   - `fern_actor_spawn(name)` mirrors `actors.start`.
   - `fern_actor_send(actor_id, msg)` mirrors `actors.post`.
   - `fern_actor_receive(actor_id)` mirrors `actors.next`.
   - `fern_actor_mailbox_len(actor_id)` returns mailbox length or `-1` for invalid actor id.
   - `fern_actor_scheduler_next()` returns next ready actor id in round-robin order, or `0` when no actor is ready.

These behaviors are regression-tested in `tests/test_runtime_surface.c` and treated as the compatibility baseline for Gate C runtime behavior.

### Milestone 7.7 Step A Memory API Contract (2026-02-06)

Fern runtime now exposes an explicit memory ownership API surface:

1. `fern_alloc(size)`:
   - Allocates managed memory.
2. `fern_dup(ptr)`:
   - Boehm backend: returns the same pointer.
   - Future RC backend: increments reference count.
3. `fern_drop(ptr)`:
   - Boehm backend: no-op semantic drop.
   - Future RC backend: decrements reference count and may reclaim.
4. `fern_free(ptr)`:
   - Compatibility alias for `fern_drop(ptr)`.

These semantics are regression-tested via runtime C-ABI harness coverage in `test_runtime_memory_alloc_dup_drop_contract` (`tests/test_runtime_surface.c`).

### Milestone 7.7 Step B RC Header and Type-Tag Contract (2026-02-06)

Fern runtime now exposes Perceus-style object header metadata for RC-managed payloads:

1. `fern_rc_alloc(payload_size, type_tag)`:
   - Allocates payload with a header carrying `refcount`, `type_tag`, and `flags`.
   - New objects start with `refcount = 1` and `FERN_RC_FLAG_UNIQUE`.
2. `fern_rc_dup(ptr)`:
   - Increments refcount for non-NULL payload pointers.
   - Clears `FERN_RC_FLAG_UNIQUE` when refcount exceeds 1.
3. `fern_rc_drop(ptr)`:
   - Decrements refcount for non-NULL payload pointers (floored at 0).
   - Marks `FERN_RC_FLAG_UNIQUE` when refcount returns to 1.
4. Metadata accessors:
   - `fern_rc_refcount(ptr)`, `fern_rc_type_tag(ptr)`, `fern_rc_flags(ptr)`, `fern_rc_set_flags(ptr, flags)`.
5. Core runtime type tags:
   - `fern_result_ok/err` allocations are tagged `FERN_RC_TYPE_RESULT`.
   - `fern_list_new/with_capacity` allocations are tagged `FERN_RC_TYPE_LIST`.
   - Runtime `FernStringList` allocations are tagged `FERN_RC_TYPE_STRING_LIST`.

These semantics are regression-tested via runtime C-ABI harness coverage in `test_runtime_rc_header_and_core_type_ops` (`tests/test_runtime_surface.c`).

### Milestone 7.7 Step C Codegen Dup/Drop Contract (2026-02-06)

Fern codegen now inserts initial ownership operations for a constrained subset:

1. Pointer alias let-bindings:
   - `let y = x` where `x` is tracked as pointer-owned emits `fern_dup(x)` at bind site.
2. Function-scope owned pointer names:
   - Pointer parameters and pointer let-bindings are tracked as owned names.
   - `fern_drop(name)` is emitted at function return sites for tracked owned names.
3. Returned-identifier preservation:
   - If the return value is an owned identifier (including block-final identifier), that identifier is excluded from drop emission at that return site.
4. Scope note:
   - This is an intentionally constrained Step C baseline and does not yet provide full ownership inference across all control-flow shapes.

These semantics are regression-tested in `tests/test_codegen.c` via `test_codegen_dup_inserted_for_pointer_alias_binding` and `test_codegen_drop_inserted_for_unreturned_pointer_bindings`.

## Unreleased Directory Listing Migration (2026-09-05)

Decision 50 changes `fs.list_dir(path)` and `File.list_dir(path)` from
`List(String)` to `Result(List(String), Int)` in both frontends. This is an
explicit breaking source change for the unreleased migration, not a compatible
patch/minor change. A release carrying it must identify the break and follow
the major-release and migration-note requirements above.

Replace `let entries = fs.list_dir(path)` followed by List operations with a
match on `Ok(entries)` / `Err(code)`, or use `fs.list_dir(path)?` inside a
function returning a compatible Result. Empty directories produce `Ok([])`.
Missing paths return code 1, permission failures 2, non-directories 5, and
other open/read/close or enumeration-limit failures 3. No partial list is
reported as successful. The legacy nullable C helper remains available for
existing C callers; newly compiled Fern source uses the Result helper.

## Deprecation Lifecycle

Every removal or incompatible behavior change must follow this sequence:

1. Announcement: document deprecation in release notes and migration guidance.
2. Warning phase: compiler or docs include explicit deprecation warning and replacement.
3. Removal: remove behavior only after the support window is met.

Minimum support window: at least **2 minor releases** after first deprecation warning.

Required deprecation notice content:

1. What is deprecated.
2. Recommended replacement.
3. Earliest release where removal may happen.

## Versioning Policy

Fern uses Semantic Versioning aligned with `include/version.h`.

1. `MAJOR`: incompatible language/stdlib/CLI changes.
2. `MINOR`: backward-compatible features.
3. `PATCH`: backward-compatible fixes.

`include/version.h` is the single source of truth for version numbers.

## Automated Release Flow

Fern uses conventional commit messages with `release-please` to automate:

1. Semver bump calculation (`major`/`minor`/`patch`) based on commit intent.
2. Release note/changelog generation.
3. Version updates in tracked files (including `include/version.h`).
4. Creation of version tags (for example: `v0.2.0`), which trigger the release packaging workflow.

Workflow/config files:

1. `.github/workflows/release-please.yml`
2. `.github/release-please-config.json`
3. `.github/.release-please-manifest.json`

## Executable Feature Boundaries

The [release readiness checklist](RELEASE_READINESS.md) separates working APIs
from design targets. The [actor contract](ACTOR_RUNTIME.md) defines the tested
mailbox and supervision behavior. Native compilation rejects unimplemented
spawn/spawn_link/receive execution rather than emitting misleading executables;
parse/check retain planned syntax for tooling. This corrects previously silent
miscompilation and does not claim the complete concurrency model is available.

## Release Checklist

Before any tagged release:

1. Run `mise run check` and every gate in [release readiness](RELEASE_READINESS.md).
2. Run `mise run perf-budget` (or `PERF_BUDGET_FLAGS=--skip-build mise run perf-budget` if release build already ran in the same job).
3. Run `mise run release-policy-check`.
4. Publish release notes with:
   - compatibility notes,
   - deprecations and removals,
   - migration guidance.

For normal releases, tags and release notes are produced by `release-please`.
Manual tags should only be used for exceptional recovery flows.

The release workflow must fail if policy checks fail.

## Workflow Linkage

The GitHub release workflow references this policy via `mise run release-policy-check`.
This ensures compatibility/deprecation requirements are evaluated on every release run.
