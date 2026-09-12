> Historical document, retained for architectural context. Its implementation paths, commands and measurements describe the retired setup. See [the current build guide](../../BUILD.md) and [roadmap](../../ROADMAP.md).

# Incremental Rust migration

The default build and installation now select the Rust compiler as `fern`, with
`fern-c` retained as the explicit C bootstrap/reference compiler. QBE remains the
default native backend; Cranelift is an optional separately tested backend.
[The completion contract](RUST_DEFAULT_MIGRATION.md) records executable language,
Result proof, tooling and distribution acceptance. The dated sections below are
historical checkpoints; their opt-in/default claims and counts describe those dates.
The original [evaluation](RUST_FRONTEND_EVALUATION.md) measures the initial scalar
prototype, not the expanded compiler.

## Shared native lowering and experimental Cranelift — 2026-09-06

The Rust frontend now lowers checked language semantics into one structured
machine representation. QBE serialization and the optional Cranelift object
backend consume the same operations, preserving the existing closure, tail-call,
fault/defer, actor and JSON descriptor paths. Compiler-owned helper routines are
structured Rust builders; no parser translates emitted QBE text into Cranelift.
Float printing and interpolation use fixed-signature runtime calls so generated
code does not depend on a variadic calling-convention workaround.

```sh
mise run rust-cranelift-build
./bin/fern-rs-cranelift run --backend=cranelift examples/tiny_cli.fn
./bin/fern-rs-cranelift build --backend=cranelift examples/tiny_cli.fn -o hello-cranelift
mise run rust-cranelift-check
```

The separate executable leaves the ordinary `bin/fern-rs` available. QBE remains
the default, even when the feature is built. Backend selection currently applies
to `build` and `run`; `emit`, source tests and preview packaging retain their
QBE contracts. The experimental feature pins Cranelift 0.135.1 under the dated
nightly and checked-in Cargo lockfile (Decision112).

A selected Cranelift build emits the native object directly and does not invoke
QBE or an assembler. Final linking still requires the host toolchain, Fern's C
runtime and native libraries. The C reference compiler, native supervisor,
Tree-sitter C parser/scanner and Python correctness oracles have separate roles;
changing code generators does not replace them.

The complete Cranelift feature gate passes on macOS ARM64 and Linux ARM64,
including 293 independent native output oracles per platform, feature Clippy
and Rust tests (1,559 on macOS; 1,562 on Linux). Targeted tests cover mixed
integer/Float ABI arguments, loop phis, relocations and forced-GC retention. Source-level debugger acceptance,
controlled end-to-end performance measurements and default promotion remain open.
See [the backend assessment](BACKEND_REASSESSMENT.md) for current scope and the
acceptance policy. Earlier checkpoint counts below remain historical records.

## Collections and error values — 2026-09-05

- Recursive concrete `List(T)`, `Option(T)`, and `Result(T, E)` types, including
  nested combinations and Unit payloads.
- Local constructor/empty-list inference with concrete type resolution before
  lowering. Underconstrained values require annotations instead of guessed types.
- Immutable list construction and core operations; scalar contains uses value
  equality, including String contents rather than pointer identity.
- Exhaustive matches with scalar literals, catchalls, Some/None/Ok/Err patterns,
  scoped payload bindings, and multiline arm bodies.
- Postfix `?`, which continues with an Ok payload or returns the Err value from
  its helper immediately, checking compatibility with the enclosing error type.
- Diagnostics for discarded Result expressions, unused Result-bearing bindings,
  parameters, named pattern bindings and terminal aliases, nonexhaustive/unreachable
  arms and type mismatches. General
  path-sensitive ownership analysis remains open.
- Multiline calls, lists, and type arguments, with explicit source/token/type/depth
  limits and an occurs check against recursive inferred types.

Try the collection and error examples:

```sh
mise run rust-build
./bin/fern-rs run compiler-rs/tests/collections/lists.fn
./bin/fern-rs run compiler-rs/tests/collections/propagation.fn
./bin/fern-rs run compiler-rs/tests/collections/nested_sums.fn
```

The propagation example prints `continued`, then `four`, then `division by zero`.
The failure returns before the second `continued` could execute. Main explicitly
matches the helper's Result; Rust main still returns Int or Unit.

## Custom types, modules, and formatting — 2026-09-05

- Nominal sum and record types, recursive generic layouts, field access, and
  bounded concrete specialization of generic functions.
- Nested pattern coverage, guarded arms with scoped bindings, and tag checks
  before loading payload fields. Guards never count toward exhaustiveness.
- Bounded module loading with aliases, selective imports, reexports, visibility,
  cycle checks, and diagnostics attributed to their original source files.
- A Rust formatter with comment preservation, idempotence, complete structural
  validation, and atomic updates. It never invokes the C formatter.
- Six native applications cover recursive trees, full-width record payloads,
  contextual generic inference, guard evaluation order, and a multi-file project.

At this historical checkpoint, generic bodies were checked only when concretely
instantiated. The generic-definition checkpoint below closes that gap.

## Numeric, application, and interactive expansion — 2026-09-05

Float arithmetic/comparison/printing retains IEEE doubles through generic payloads;
signed zero and NaN behavior are checked natively. Structural tuples support nested
patterns, destructuring and projections. Pipes preserve evaluation order, and
interpolation supports nested expressions with typed scalar conversion.

The runtime registry describes source signatures and their physical transport.
String-list, packed byte Option, regex tuple and process-result adapters copy into
the Rust layouts explicitly. Substring offsets bypass the old narrowing ABI.
Opaque terminal objects cannot be fabricated from user records or integers.
The process runtime now executes `System.exec_args` as literal argv with
`posix_spawnp`, fixing an existing quote-expansion allocation bug.

A persistent typed-IR REPL retains successful bindings without replaying effects.
The Rust LSP tracks UTF-16 edits and imports across unsaved buffers. Both enforce
resource limits and recover from invalid user entries. These tools do not yet
establish complete editor or runtime parity.

## Runtime representation

Semantic types stay distinct throughout IR even when they have identical machine
widths. Lists and Results reuse the existing C runtime. Option uses that runtime's
heap-backed Result helpers internally, mapping Some to Ok and None to Err with an
unused zero payload. This avoids the old packed Option representation, which
truncates payloads to 32 bits. Native tests retain full Int values and String/list
pointers, including nested sum values and 1,500 allocations.

The shipping C Option ABI is unchanged. Rust uses an explicit adapter for the
lossless packed byte result from `String.char_at`; substring offsets use a
full-width implementation. Further packed Option APIs still require auditing. Valid-index and nonempty-list preconditions of
`List.get`/`List.head` remain unchanged; callers must check lengths before reading.

## Verification

The collections checkpoint had 94 passing Rust tests and 542 passing C tests.
The expanded checkpoint has 243 passing Rust tests, 550 passing C tests and 81 core native programs,
plus 34 rejected invalid programs. Directory contracts add four C ABI cases,
eight native programs and two Result binder regressions across both frontends.
The 192-case seeded mutation suite checks diagnostic termination, formatter
stability and unchanged output behavior. Format, clippy, C quality, documentation
and native/Python style-checker parity gates pass locally on macOS arm64.

The milestone adds 31 native programs (19 fixed, 12 seeded generated) and 22
negative compile cases. Fixed cases cover wide signed values, strings, booleans,
nested lists and sums, Unit, immutable aliases, branch-local bindings, evaluation
order, propagation, and retained pointers under repeated allocation. Invalid
programs must fail with source diagnostics and preserve an existing output file.

These are exact expected-output tests. The original 32-program differential
suite against C remains in place and currently reports four known backend
differences. The new heap Option representation is checked against Fern semantics
directly because it deliberately differs from C's packed ABI.

Run `mise run rust-check`, `mise run check`, and `mise run docs-check` to reproduce the gates.
The same Rust gates are configured in the existing Linux/macOS CI matrix; this
milestone was verified locally on macOS arm64.

## Collections-stage release measurements

`mise run rust-evaluate` passed with the expanded frontend, using the same 101-function
shared-subset fixture, 15 check/emit samples and three verified native builds per
compiler. The host is macOS arm64 with Rust 1.75 and Apple clang 16. See
[raw measurements and hashes](../reports/rust-collections-evaluation-2026-09-05.json).

| Measurement | C | Expanded Rust |
| --- | ---: | ---: |
| Check median / p95 | 2.85 / 5.10 ms | 3.46 / 4.03 ms |
| Emit median / p95 | 4.03 / 4.68 ms | 3.23 / 4.28 ms |
| Native build median / maximum | 112.00 / 112.86 ms | 101.11 / 245.06 ms |
| Compiler + QBE backend | 549,384 bytes | 982,112 bytes |

The Rust executable is 668,072 bytes and its separate QBE helper is 314,040 bytes;
the shared runtime/system libraries are excluded from both totals. Native output
is verified for every timed build, and the new collections corpus also passes
against the release compiler. These small, local samples include process startup
and lack CPU isolation. They demonstrate a usable edit/check loop, not a general
speed advantage or full-language parity. This synthetic fixture does not measure
collection-heavy compilation separately.

## Migration acceptance and future work

See [the default migration contract](RUST_DEFAULT_MIGRATION.md),
[executable language parity](../LANGUAGE_PARITY.md), [tooling parity](../TOOLING_PARITY.md)
and the [active roadmap](../../ROADMAP.md). Generalized actor suspension, broader
server/database APIs and additional editor grammar remain future language work.
The compiler migration preserves the separate C runtime, QBE backend, native
supervisor, C reference compiler and correctness oracles.

Directory native parity exposed a C backend defect: matched Result payloads were
classified as Strings solely because their storage was 64 bits. Pattern binders and expressions
now retain checker-owned semantic types through emission, with all AST factories
initializing that metadata and match-arm scopes restoring it. Native regression
coverage distinguishes integer error arithmetic from String error concatenation;
this correction also supports the explicit directory Result migration in Decision 50.
Result constructors explicitly extend integer payloads or bitcast floating
payloads into the native full-width representation. Checked expression types
also determine function argument/result widths and prevent an outer pattern
binder from changing the type of a shadowing inner let expression.

The native bootstrap gate also exposed multiline match arms being parsed as single
expressions. C now retains their statement blocks, consumes only owned indentation
boundaries and stops after parse errors. Seven regressions cover mixed branch layouts, nested arms,
condition-only matches, following declarations and malformed-input termination.

## Functions and higher-order execution — 2026-09-05

The typed pipeline now infers anonymous functions, specializes named function
values and lifts escaping closures into explicit environments. Native generated
functions share one hidden-environment convention; runtime/builtin function values
use concrete wrappers. List map/fold/filter/find/any/all and Option/Result callbacks
execute typed calls, preserving Float payloads, short-circuiting and source order.
Empty list output uses a valid runtime allocation capacity.

Interactive closures retain their originating immutable program so later entries
cannot change function identities. Unique code and captured graphs share an
aggregate storage budget. The REPL accepts open calls and commented closure headers.
The formatter and module loader support block callbacks and nested function types.
Result-bearing captures currently receive an explicit diagnostic pending delayed
ownership tracking; callbacks may return and propagate Results normally.

Six native programs cover escaping/independent/nested captures, 512 retained closures,
full-width and Float callbacks, records and all ten higher-order operations. Six
invalid programs exercise type and obligation diagnostics. Additional checker,
parser, emitter, formatter, module and interactive regressions protect the pipeline.

Closure checkpoint gates pass locally: 287 Rust tests, 550 C tests, 87 core
native programs, 40 invalid programs, dual-frontend directory/Result contracts,
192 seeded mutations, formatting/clippy and documentation checks.

## Maps and record updates — 2026-09-05

Immutable maps support Int/Bool/String keys and arbitrary concrete values through
source parsing, contextual inference, generic specialization, native execution,
formatting, modules and interactive sessions. Compiler-owned helpers reuse GC
allocation and list primitives; they do not add a C Map ABI. String equality uses
contents. Duplicate keys keep their insertion position and last value. Public
updates preserve existing aliases, and empty maps use valid allocation capacity.
The initial search/update implementation is linear.

Record updates now construct changed records, evaluating the base and every field
expression once in source order. This is verified against explicit native output;
the C backend's unchanged-record behavior is not an oracle. Six new native programs
and nine invalid cases cover scalar keys, full-width/Float/Result/closure values,
ordered effects, missing keys, persistent aliases and retained environments.

The design document's old milestone checkmarks have been replaced by acceptance
criteria with links to the active roadmap, where remaining type-system and
standard-library work is explicit. C remains the shipping default while the
remaining language and migration milestones are completed.


## Early exits and deferred cleanup — 2026-09-05

The Rust frontend now supports explicit returns, postfix conditionals,
condition-only matches, let-else unwrapping and dynamic function-exit cleanup.
An internal bottom type identifies paths that do not produce values. Native
branches join only continuing paths; strict argument evaluation stops at the
first executed return, including inside otherwise polymorphic calls.

Deferred code is lifted into a captured Unit function. A GC-visible function
stack records registrations, and one epilogue preserves the full-width return
value before invoking cleanup in reverse order. Nested blocks retain their
registrations until the enclosing function exits. Early returns and propagated
Result errors use the same exit path. User lambdas retain independent boundaries.

Interactive evaluation preserves the same ordering and additionally attempts
cleanup after evaluator failures. An entry-wide 10,000-step cleanup budget is
separate from the 100,000 ordinary evaluation steps; each function permits at most
4,096 pending cleanups. The original evaluation error is preserved if cleanup also
fails. Retained code accounting includes cleanup and let-else branches.

Nine native programs and ten invalid cases cover skipped operands, Float/Int64
returns, conditional registrations, lexical snapshots, failure propagation,
let-else scopes, lambda boundaries and recursive captures. Interactive tests also
verify cleanup after division and work-budget failures. With blocks and collection
iteration remain the next control-flow requirements; C remains the default.


## Iteration and typed with handlers — 2026-09-05

For loops now consume one immutable List, Map or Range snapshot. Range values
store endpoints instead of allocating elements; native and interactive loops
check the inclusive endpoint before incrementing, including Int::MAX. Map loops
yield full-width key/value tuples in insertion order. List enumeration retains
semantic values, including Float and closures. Loop targets are local to each
function; break/continue preserve registered function-exit cleanup.

With blocks retain flat sequential steps and distinct typed error handlers.
Each handler checks applicable source arms in order and requires exhaustive
coverage, while preserving its error payload's real type. A failed step skips
all following initializers and the success body. Handlers cannot access success
bindings; they can return, propagate errors or control an enclosing loop using
the existing abrupt-exit rules. Omitting else uses ordinary Result propagation.

Native fixtures cover heterogeneous nominal errors, generic With specialization,
Float payloads, scope shadowing, guarded handlers, full-width range boundaries,
collection enumeration, retained range/loop captures and cleanup during loop
control. Interactive tests enforce the same semantics and bounded range work.


## Numeric domains, literal text and runtime faults — 2026-09-05

Int power uses bounded exponentiation by squaring; bitwise shifts normalize counts
modulo 64. Int minimum divided by -1 wraps consistently, and its remainder is zero.
Float power uses libm, and Float membership compares IEEE values in both direct
and first-class contains calls. Base-prefixed integer digits and separators are
validated before producing a full-width value.

Generated functions now receive an explicit fault-context pointer after their
closure environment. An escaping closure receives its current caller's context;
it never retains a pointer to a finished invocation. Guards check this context
before using call results or executing subsequent effects. Numeric domain errors
unwind through the same deferred cleanup path as other function exits. Cleanup
callbacks run with clear fault state, then restore the first failure. Native main
emits one stable diagnostic and exits 1. This changes only compiler-owned calling
conventions; the C runtime ABI and source Function/Result types are unchanged.

Triple strings preserve actual content bytes, block comments may nest within
limits, and @doc metadata remains associated with declarations. Unicode identifier
spelling is preserved without normalization. Formatting must retain text contents
and documentation. Runtime-fault fixtures separately verify cleanup/effect order,
error messages and exit status, alongside valid and rejected source programs.


## Private return inference and runtime boundaries

Private functions with annotated parameters may infer their returns before
concrete specialization. A bounded shared-constraint pass supports forward
references and recursive definitions with a type anchor, including generic
schemes established by their bodies. Unresolved recursive or generic results
require an annotation. Public signatures retain explicit return types, including
when definitions are imported; omitted main remains Unit. Retry work and
inference storage have a shared budget, rather than restarting the full budget
for every dependency retry. Parameter inference and function clauses remain open.

`main -> Result((), E)` accepts a concrete error type: Ok exits 0, Err exits 1
with `fern: main returned Err` on stderr, after cleanup. Error payload display
awaits a general display protocol. Runtime faults take precedence over entry
Results, and the first fault remains authoritative during cleanup.

List.get/head check bounds before access through direct, registry and first-class
calls. String.repeat checks its 16 MiB content cap before multiplication or
allocation and returns empty immediately for nonpositive counts or empty input.
The shared C helpers independently enforce these limits even in release builds;
Rust-generated calls additionally unwind deferred cleanup through the fault
context. These direct-valued APIs are not recoverable Result APIs yet.

String.slice retains clamped byte indexing but rejects endpoints inside Unicode
scalars. Empty-delimiter String.split produces complete scalars, including
separate combining marks, and produces no elements for empty input. A preflight
check also rejects malformed UTF-8 entering through legacy native I/O before
splitting, preserving cleanup. Nonempty-delimiter behavior remains unchanged;
consistent UTF-8 validation at all native string ingress is still an audit item.


## Shared list and tuple sequence patterns

Exact list patterns and final list/tuple rest patterns use the same AST, checked
IR, coverage analysis and execution engine in match, let-else, irrefutable
bindings, loops and with. Result discard checks include prefix and suffix values.
List lengths dominate all element reads; named suffix copying is delayed until
all nested structural checks succeed, before a guard needs its binders. Ignored
tails allocate nothing and whole-list bindings alias the original immutable list.
Tuple suffixes preserve singleton identity and use Unit for empty suffixes.

Coverage expands list shapes into conceptual empty/nonempty constructors, with
an expansion depth budget checked before building recursive matrices. The REPL
stages bindings atomically and reserves suffix-copy work before allocating, so
failed siblings or exhausted budgets cannot publish partial bindings. Retained
closure code storage also counts the new pattern variants. Named list suffixes
currently copy, making repeated recursive tail scans quadratic; persistent list
views and tail-call optimization remain separate performance work.


Embedded expression suites now preserve bounded indentation frames inside calls,
lists and tuples. Match, if, for, with and callbacks compose with inline separators
and closing delimiters; ordinary nested delimiters still suppress layout. Ten
native programs and formatting/checked-IR equivalence cover sequence bindings,
block arguments, effect order, full-width payloads, Result tails and escaping
closures. Fifteen invalid programs retain the output-file preservation contract.
An independent audit of 1,850 small Boolean/nested-list pattern matrices found no
coverage/usefulness mismatch; this supplements the bounded checker regressions.

## Function clauses and native self recursion — 2026-09-05

Adjacent typed parameter clauses normalize through the existing exhaustive match
engine before private return inference. Guards, source spans, module visibility,
function-owned return/defer behavior and Result obligations are preserved.
The formatter retains clause groups and arrow/colon syntax. Interactive `:paste`
and `:end` submit a whole group without retaining incomplete definitions.

Eligible direct self calls in return position jump to a local recursion header
only after all arguments finish. Parameter and for/with scratch slots are allocated
once in the native entry block. Million-step programs cover scalar and pointer
values, first-class entry, nested cleanup, argument order and runtime faults.
Owned defer registration disables frame reuse; indirect/mutual recursion and
interactive evaluation retain their existing call behavior. General proper tail
calls and zero-copy list suffixes remain future work.

This checkpoint passes 541 Rust tests, including the backend's independent
255-parameter coverage checks, and retains 550 passing C tests. New native
coverage includes seven clause programs, seven successful recursion programs
and one controlled-fault recursion program, plus 11 invalid inputs.

## Pattern-anchored private parameters — 2026-09-05

Omitted private parameter annotations now collect evidence across the entire
clause group before normalization. Literals, nested sequence/sum patterns and
nominal constructor schemas determine types independently of callers. Tuple-rest
constraints wait for known arity; later annotations can supply declared generics.
Public omissions, conflicting evidence and still-ambiguous types report diagnostics.
The pass uses a separate aggregate 400,000-operation inference budget across groups,
in addition to the existing source/type/depth and return-inference bounds.

Seven native programs cover recursive dispatch, inferred container payloads,
constructor schemas, tuple-rest ordering, independent generic instantiations and
an imported private helper. Nine invalid programs and two interactive regressions
check rejection and session recovery. Whole-signature generalization remains open.

The pattern-inference checkpoint passes 554 Rust tests, 550 C tests and the
complete native, fuzz and documentation gates on macOS arm64.

## Generic definition schemes — 2026-09-05

Every generic body is checked before specialization, including unused functions,
nested callbacks and pattern coverage. Declared variables obey rigid type equality;
a literal cannot satisfy an arbitrary promised generic return. Intrinsic operation
requirements preserve generic arithmetic, scalar display, membership and map keys,
and propagate through calls, function values and recursive helpers. Nominal field
requirements prevent invalid Map key types from hiding inside record signatures.

The solver performs one body check per generic definition and closes deduplicated
requirements under the existing shared 400,000-operation inference budget. Only
surviving finalized calls contribute requirements, preserving early exits. Concrete
specialization remains a second validation boundary, including conditional Result
restrictions. Full source-level traits/constraints and private signature generalization
remain separate milestones.

Four native programs cover parametric helpers, capabilities, callbacks, closures,
recursive requirements and nominal fields. Sixteen invalid CLI programs and three
interactive regressions check early rejection and preserved session state.

Generic scheme checkpoint gates pass: 579 Rust tests, 550 C tests, complete
native/fuzz checks and documentation checks on macOS arm64.

## Source navigation and completion

The Rust language server now advertises go-to-definition and completion alongside
its existing synchronization and diagnostics. Definition locations use original
source identities, preserving separate clause binders, local shadowing, captures,
constructor/type references and visible imported names. Unsaved dependencies,
module aliases and public re-exports use the compiler's module-resolution facts.

Each request builds a fresh bounded index from accepted editor buffers. Syntax
errors never reuse an older source location; type errors do not prevent lexical
navigation through a successfully parsed graph. Lexer spans exclude comments and
literal text while retaining code inside interpolation. Both cursor positions
and replacement ranges use UTF-16, including non-BMP identifiers.

Completion filters by lexical scope and prefix, with deterministic ordering and
limits of 256 entries and 1 MiB of serialized output. Builtin prefix suggestions
also work for incomplete expressions such as `List.`. Ordinary compilation does
not retain editor-only symbol copies. Typed hover, record-member completion,
rename, code actions and broader incomplete-code recovery remain open.

Retained editor symbol names have an aggregate 8 MiB byte limit, in addition to
the entry count bound. This bounds the new editor copies; the existing resolver
still has a separate transient alias-expansion limitation.

The checkpoint adds 21 navigation regressions and three module/index regressions.
An executable JSON-RPC smoke verifies initialization, definition, scoped
completion, shutdown and clean protocol output.

Reviewed navigation checkpoint gates pass: 603 Rust tests, 550 C tests, complete
native/fuzz gates and documentation checks on macOS arm64.

## Parser-based source documentation

`fern-rs doc source.fn` now generates Markdown from the Rust parser. `--html`
produces a standalone page, and `-o` installs either format atomically. Invalid
syntax leaves existing output intact; source aliases cannot be overwritten.
Generation needs neither main nor the native backend and executes no user code.

Functions retain original headers, including pattern clauses, guards, nested
function types and omitted annotations. Clauses share one documentation entry;
types retain their fields and variants. Literal @doc metadata is attached by
source position to its owning declaration. HTML escapes source, titles and doc
text; Markdown uses source-safe code fences and retains authored doc markup.

The initial command accepts one file and includes private/public declarations.
Input is limited to 1 MiB and 4,096 declarations; output is limited to 8 MiB.
This does not yet provide directory navigation/search, inferred documentation
signatures or executable Rust doc tests. Fifteen new library/CLI regressions
cover source syntax, ownership, escaping, resource limits and output preservation.

Source-documentation checkpoint gates pass: 618 Rust tests, 550 C tests, the full
native/fuzz suite and documentation checks on macOS arm64.

## Core whole-private-signature inference

The Rust checker now infers omitted private parameters and returns from all
clause patterns and function bodies before consulting callers. It analyzes
lexical dependencies with an iterative graph traversal and solves recursive
components callee first. Unfinished recursive members share type constraints;
completed named functions instantiate independent schemes at each reference.
Identity, generic list recursion, apply/compose, returned closures and nominal
constructor helpers work without explicit private parameter annotations.

Intrinsic requirements remain attached to generalized definitions: an inferred
addition helper supports Int, Float and String, while inferred multiplication
retains its numeric restriction. Literal evidence can establish a concrete type.
Return-only None/empty-list schemes receive independent contextual payload types.
Local aliases remain monomorphic; explicit universal annotations remain rigid.
Public boundaries, unanchored recursive returns, occurs checks, Result obligations
and concrete backend validation remain enforced. Fully annotated parameter
components preserve the existing generic return-inference behavior.

One 400,000-unit inference budget spans dependency traversal, pattern/body
constraints, recursive solving, generalization and final generic requirements.
The former repeatedly retried forward chain now resolves in dependency order.
Declared source/type/depth limits still apply before internal inference slots.
Diagnostic type descriptions use a bounded source renderer, stripping ownership
only from semantic compiler-owned rigid variables and retaining the distinction
between separate type parameters with the same original spelling.

Core acceptance adds eight native programs and 14 invalid programs, including
both caller orders, mixed annotated/inferred recursion and imported helpers.
Three REPL regressions cover retained generic definitions, callbacks and rollback.
The earlier identity/list-length rejection fixtures become positive coverage.
Delayed field/update/iteration/tuple-rest evidence from later expressions remains
an open checkpoint; unknown record shapes and tuple arities are never guessed.

Core signature checkpoint gates pass: 670 Rust tests, 550 C tests, 166 core native
programs, 21 entry/access programs, 148 invalid inputs and the complete fault,
fuzz and documentation suites on macOS arm64.

## Delayed private-signature shapes

Field access, record updates, tuple-rest bindings and iteration now gather later
body evidence through bounded structural obligations. Evidence can come from
either branch, a callback context or another use of the same value. Exact tuple
suffix evidence works in both directions, and receiver enumeration remains
distinct from a record callback named enumerate. Unknown shapes remain errors.

Inference uses explicitly tagged, non-executable probes with crate-private
construction. After settling obligations, the original source is rechecked with
its generalized signature. Final IR publication, QBE and REPL storage/execution
reject surviving probes, including unreachable children. Constraint storage,
field lookup, substitution and repeated settling share the inference work budget.

Fourteen checker regressions, six internal boundary/budget checks and a
compile-fail construction test cover the implementation. Seven native programs
and eight invalid programs verify output order, callbacks, guards, cleanup and
Result handling. Two interactive regressions cover retained generic closures,
iteration effects and rollback after rejected entries.

Delayed-shape checkpoint gates pass: 693 Rust tests, 550 C tests, 173 core native
programs, 156 invalid inputs and the complete entry/access, controlled-fault, fuzz
and documentation suites on macOS arm64.

## Native JSON core

The runtime now provides an immutable opaque JSON tree with validating parsing,
ordered compact serialization, typed accessors and exact number text. Explicit
Int conversion rejects fractions/overflow; Float conversion preserves binary64
bits and uses a private locale. Unicode validation, decoded duplicate-key
rejection, escaped NUL preservation and explicit resource limits are documented
in [the native contract](../JSON_NATIVE_CORE.md).

This checkpoint preserves both legacy JSON symbols and current source bindings.
Fourteen new ABI symbols remain explicitly internal until the source-type and
adapter migration lands. Debug, release and ASan/UBSan runs each pass 14,309 API
checks, 24 internal budget checks and 6,000 deterministic numeric oracle cases.
The new native gate runs with the full Rust suite.

The integrated native JSON checkpoint passes all C/Rust/native/fuzz/documentation
gates on macOS arm64; existing source-language behavior remains compatible.

## Directory documentation

Parser-based documentation now accepts directories and produces one deterministic
Markdown or standalone HTML artifact. HTML provides module navigation and local
search over module names, original signatures and literal documentation. Numeric
anchors separate duplicate declaration names, and all untrusted markup remains
escaped data. Search only reads text and changes visibility; no external assets
or service are needed, and all documentation remains visible without scripting.

Discovery excludes hidden/build/dependency entries and never follows child
symlinks. Files, directory entries/depth, paths, source bytes, declarations and
rendered output have explicit limits. Every input is checked before atomic output
publication, including hardlink aliases of any source. Seven new regressions
cover rendering, discovery, errors, limits and literal Unix backslash paths.
Browser checks verify declaration filtering, empty results and navigation after
a filter. Executable doc tests and inferred documentation remain open.

Directory-documentation gates pass: 700 Rust tests, 550 C tests and the full
native, JSON, fuzz and documentation suites on macOS arm64.

## Checked editor facts and typed hover

Optional source fact recording now publishes only after ordinary checking,
specialization and probe rejection succeed for the current overlay graph. A
selected source function is revalidated with its closed scheme; generic declaration
signatures remain distinct from instantiated uses and local binding types. Source
anchors preserve clause binders, captures, destructuring and actual field origins.
Ordinary compilation allocates no recorder.

Hover provides plaintext signatures, intrinsic requirements and literal @doc
excerpts. Valid-source completion adds checked record/tuple fields and supported
receiver methods, including arbitrary checked receiver expressions. Separate type
and value indexes use actual parsed annotation spans, preserving same-named
function/type identities; doc ownership follows declaration position. Inference
variables and generated backend names never define public hover text.

Metadata has independent node/name bounds; rendered fragments, documentation
excerpts, JSON escape expansion and completion output are separately bounded.
Twenty-six new metadata/editor regressions cover current overlays, UTF-16,
requirements, bindings, namespaces, escaping and resource limits. Typed member
recovery for incomplete source remains a separate checkpoint.

Typed-editor checkpoint gates pass: 726 Rust tests, 550 C tests and the full
native/JSON/fuzz/documentation suites on macOS arm64. An executable LSP protocol
smoke also confirms hover capability, inferred signatures and documentation.


## Transparent type aliases

Rust now expands scalar and generic aliases before nominal registry construction
and private-signature inference. Substitution is simultaneous and capture-free;
forward aliases, nested containers, callback annotations and nominal recursive
records retain their target semantics. Transparent cycles, unknown types, invalid
arity, conflicting declarations and excessive depth/node/work growth are rejected
before retaining expanded trees. Unicode capitalized names follow source rules.

Aliases add no constructors or privacy boundary. Module imports/reexports retain
visibility checks; formatting, documentation, type navigation and checked hover
retain original declarations. The REPL uses final layout field types when showing
aliased nominal payloads. Distinct newtypes and unions remain separate work.

The checkpoint adds 26 Rust regressions, seven native output cases and 12 invalid
programs with output preservation, plus alias inputs in bounded mutation testing.

Alias checkpoint gates pass: 752 Rust tests, 550 C tests, 180 core native
programs, 168 invalid inputs and the full entry/access, controlled-fault, JSON,
fuzz and documentation suites on macOS arm64.


## Module reference identity

Resolved imports now carry explicit global identities through function values,
direct calls, pipes, captures and dependency discovery. An unrelated local named
`model` no longer captures `m.value` after `import model as m`. A local named `m`
still shadows that source alias, and arguments retain ordinary lexical checking.
Original spelling stays available to source tools; both names and nested
arguments are bounded by the frontend preflight checks. Alias expansion visits
annotations inside resolved call and pipe arguments.

Eight Rust regressions and four native output cases cover identity, source
spelling, inferred callers, alias annotations and real record-field shadowing.
Four native rejection cases preserve existing output and local type diagnostics.

Module-identity checkpoint gates pass: 760 Rust tests, 550 C tests, 184 core
native programs, 172 invalid inputs and the complete remaining gate suite.


## Rust native JSON migration (J2)

Following the J1 native foundation, Rust now exposes24 `json` functions through
opaque `json.Value`/`json.Error` types, with `Json` aliases. Immutable builders,
exact conversions and lossless members/elements access use explicit native ABI
adapters. The Float builder uses C double/QBE d; members convert native two-word
records into tagged Fern tuples only after successful Result inspection. Object
builders evaluate their Map input once and reserve bounded parallel-list storage.

The [source contract](../JSON_RUST_API.md) identifies the unreleased Rust signature
breaks. The C source JSON contract and legacy copy symbols remain unchanged;
The J3 checkpoint below adds REPL parity; at J2 these calls were explicitly
unavailable during interactive evaluation. Typed encode/decode and derive(Json)
are still future work. The acceptance
corpus adds ten native output programs and twelve semantic rejection programs;
eight Rust tests cover aliases, formatting, opaque/public-IR validation and REPL
refusal. Native debug/release/sanitizer tests add248 builder checks, including
actual exact16 MiB output and shared-DAG/depth rejection.

Integrated JSON source gates pass: 768 Rust tests, 550 C tests, 194 core native
programs, 184 invalid inputs and the full JSON sanitizer, fuzz and documentation
suites on macOS arm64.


## Executable documentation tests

`fern-rs test --doc` now extracts closed Fern fences from parser-owned literal
documentation and executes checked native examples. Trailing expectations are
ordinary patterns, including Option/Result wildcards, with exactly-once subject
evaluation. Source module imports/private helpers and original entry function IDs
are preserved. Each example owns an independent local scope and native process.

The checker now distinguishes fully validated library graphs from executable
entry requirements. Libraries can have no main; missing names still fail, and
existing main signatures retain their contract. Documentation tests select the
checked Int harness only after module resolution and ordinary body validation.
No source-visible placeholder can capture a local or selected imported main.

Extraction bounds count/source bytes and remaps generated diagnostics to owning
documentation. The CLI bounds runtime and captured streams, closes stdin and
cleans up private Unix process groups, including descendants inheriting output.
Nine native scenarios plus independent/imported examples cover exact patterns,
once-only effects, failures, timeouts, output limits and original Int/Unit/Result
entry behavior. Twenty Rust tests cover preparation, lexical markers, Unicode,
source identity, library validation, offline CLI behavior and process capture.

Documentation-test checkpoint gates pass: 788 Rust tests, 550 C tests, the
complete native/JSON/fuzz suites and documentation checks on macOS arm64.
Frontend-only Cargo tests require no C/QBE backend; native example execution
is verified separately by the explicit rust-check gate.


## Interactive JSON parity (J3)

All 24 dynamic JSON operations now execute in the Rust REPL through a std-only
immutable value representation. The parser, exact conversions, ordered objects,
NUL behavior, errors and per-operation limits match the native contract. Native
C source migration and typed codecs remain separate.

Six Session tests reuse all ten native output programs and verify retained values,
closures, failure rollback and shared graph storage. Fifteen engine tests cover
caps, ordinary errors, independent cleanup budgets, exact output boundaries and
12,000 independent numeric/formatter oracles. Decision 77 records additional
64 MiB normal and 8 MiB cleanup aggregate work/allocation limits; retained graphs
remain under the existing 16 MiB/200,000 limits with unique Rc identity counting.

Interactive JSON checkpoint gates pass: 809 Rust tests, 550 C tests and the
complete native, sanitizer, fuzz and documentation suites on macOS arm64.


## Incomplete member completion

The editor now recovers one current lexer/parser member selector without changing
source bytes. Independently concrete receivers in fixed concrete function groups
provide record fields, tuple slots and supported List methods. Missing results
stay local to an explicit non-executable editor proof; they cannot infer receiver
types or enter reusable schemes, specialization, QBE or retained REPL programs.

Twenty-one new Rust regressions cover current overlays, Unicode/CRLF/interpolation,
original edits, scopes/aliases/global identities, unrelated errors, source limits
and unreachable/public IR boundaries. The review also removed synthetic library
main declarations, so invalid free main references cannot acquire typed facts.
The loader retains exactly the bytes from which its entry AST was parsed.

Incomplete-completion checkpoint gates pass: 830 Rust tests, 550 C tests and the
complete native, sanitizer, fuzz and documentation suites on macOS arm64.


## Checked inferred documentation

The explicit `doc --inferred` mode checks current library graphs once per source
root and collects bounded reusable function schemes from finalized source facts.
Generated docs keep original headers, patterns, guards, aliases and literal text,
then add resolved signatures and intrinsic requirements using the same generic
display identities as hover. Default source-only docs retain their old contract.

Imported declarations are attributed by canonical source path and byte anchor.
Directory output shares existing navigation, local search, escaping and atomic
publication. Every documented source and loaded dependency is protected against
replacement, including hardlinks. Graph and copied snapshot work have aggregate
limits distinct from each graph's ordinary compiler limits. External presentation
metadata is bounded before copying and must match exact original source anchors.

Ten new Rust regressions cover generic schemes, aliases/clauses/doc ownership,
invalid source behavior, metadata bounds, imported signatures, atomic dependency
protection and 129 independent files without conflating cache and graph limits.
Review regressions reject mismatched metadata names/arity/clauses and ensure modest
projects do not repeatedly copy every unrelated snapshot for each module graph.
Snapshot contents are borrowed; exact entry bytes remain paired with their parsed AST.
Externally supplied metadata must remain unmodified from the same source snapshot;
size and identity validation do not authenticate caller-edited semantic types.

Browser verification confirms checked signatures and requirements are visible,
and searching requirements filters to the correct module and declaration.

Checked-documentation checkpoint gates pass: 840 Rust tests, 550 C tests and the
complete native, sanitizer, fuzz and documentation suites on macOS arm64.


## Distinct unboxed newtypes

Generic newtypes preserve nominal identity throughout checking and use their exact
payload representation in native code. Wrap/Unwrap add no allocation or tag access;
Float payloads retain full 64-bit ABI through parameters, results, closures and
collections. Scalar equality and supported Map keys use underlying semantics only
after exact identity checking. Wrapped Result obligations remain visible.

Forty-five new Rust regressions cover source checking, unboxed layout validation,
Unicode names, formatter/docs/LSP ownership, incomplete member recovery, retained
JSON wrappers and private IR rejection. Thirteen native programs cover primitive
limits, constructor callbacks, recursive containers, String keys, cleanup and
wrapped JSON; fourteen invalid programs preserve existing output. Native allocation
oracles compare equivalent wrapped and unwrapped code.

Unions and general traits remain separate. Module type/value namespace separation
is covered by the later checkpoint below. See [newtype semantics](../NEWTYPES.md)
and Decision 79.

Newtype checkpoint gates pass: 885 Rust tests, 550 C tests and the complete native,
sanitizer, fuzz and documentation suites on macOS arm64. Editor support regeneration
also passes; the existing manually maintained indentation grammar remains separate
from the generated highlights and needs broader syntax parity.


## Source-owned unit tests

Normal `fern-rs test` now executes named test_ groups alongside documentation
examples; --doc selects only documentation. Tests use real module/private scope,
source anchors and independent native processes. Unit and Result(Unit,E) entries
are eligible; generic, parameterized and other result signatures fail explicitly
without skipping later tests. Eligibility uses one ordinary checker pipeline and
only the selected source signature, avoiding unrelated editor metadata limits.

The shared entry renamer preserves original main calls and rejects malformed public
documentation entries before mutation. A dedicated QBE test mode rejects invocation
of the resolved process-exit API through direct calls, helpers or function values;
unused application exit functions and ordinary emission retain their semantics.
This prevents successful process termination from bypassing remaining expectations.

Fifteen Rust regressions cover discovery, generic specializations, source eligibility,
large libraries, immutable failed selection, offline CLI behavior, emitter mode and
newtype capability/error integration. Fifteen native cases plus import, directory
and doc-only scenarios verify effects, real-main calls, failure continuation, cleanup,
timeout/output limits, early exits and source preservation. Assertion libraries,
benchmarks, coverage and watch remain separate work.

Unit-runner checkpoint gates pass: 900 Rust tests, 550 C tests and the complete
native, sanitizer, fuzz and documentation suites on macOS arm64.


## Independent module namespaces

Types and values now have independent declaration, import and visibility maps.
A public alias or nominal owner does not publish a private same-named function;
a public function does not publish a private same-named type or its constructors.
An alias may share its spelling with a real function or unrelated constructor,
but an alias alone cannot construct a value. Same-namespace collisions still reject.

Selective imports and public reexports preserve both public identities. Type
annotations ignore local value shadowing; calls, pipes and function references
honor the original lexical receiver. Editor navigation uses exact current-source
locations, including both identities for an import selector. Formatting and
documentation use declaration provenance rather than a shared spelling list.
Combined metadata accounting preserves the existing symbol and byte limits.
See Decision 82.

Nineteen new Rust regressions and twelve native programs verify these behaviors.
Twelve invalid builds preserve existing output bytes, mode and modification time.
The deterministic mutation corpus now also includes newtype programs.


## Native quality checker under both frontends

The Fern-written quality checker now uses explicit returned accumulators for
files, violations, assertion counts and documentation rules. Rebinding inside an
inner scope no longer silently loses results under Rust. It also uses syntax
accepted by both parsers. A separate lexer fix preserves complete C lookahead and
rollback state, removing parsing dependence on earlier nested calls.

The shared parity gate accepts `--compiler` and verifies exact strict/lenient
records, severity, exit codes and file counts on five pinned fixtures, nested
folders, literal paths and all compiler/library source. An isolated failed-build
scenario verifies continuation and final failure without invoking project builds.
`mise run rust-check` runs this gate with the Rust frontend; `mise run style-parity` uses C.

This is diagnostic and source-portability parity. Full build/test/git/CLI workflow
parity, bounded native process execution and the default-checker switch remain
open. Python remains the shipping quality gate.


## Finite unions and typed narrowing

Ordinary-type unions now normalize after alias expansion and generic substitution,
including singleton collapse. Declared assignment contexts insert checked member
or subset conversions. Repeated generic equations and inferred branch joins stay
exact, independent of argument order. Typed match binders and wildcards narrow
members or subsets, with guards and generic specialization preserving coverage.

Native tagged carriers preserve all 64 payload bits and remap subset tags after
single evaluation. REPL carriers retain member identity and bounded storage.
Result handling, module namespaces, source tooling, test entries and private IR
validation all include unions. See [the contract](../UNIONS.md) and Decision 83.

The checkpoint adds 87 Rust regressions, 27 native programs, 23 parsed atomic
rejection cases and unit/main Result entry checks. The integrated gates cover
1,006 Rust tests and 553 C tests. Constructor refinements, variance, implicit
joins, lifted capabilities and complete editor grammar parity remain open.

The pinned editor parser now includes union types and typed narrowing. Fourteen
additional valid sources, four recovery cases and five incremental edits establish
function-type precedence, subset patterns, module aliases and query captures. All
38 accepted source trees agree between native and WASM parsers, with the Rust
frontend checking their actual types. Other syntax and Zed packaging remain open.

## Bounded native process capture — 2026-09-06

`System.exec_args_bounded` captures literal argv with independent UTF-8 stdout and
stderr, explicit time/byte limits and a fallible Result. Both frontends preserve
full-width limits through helpers and function values. Rust adapts only a successful
native tuple; errors keep their original payload. [The process contract](../PROCESS_EXECUTION.md)
documents exact limits, error codes, PATH behavior and cleanup boundaries.

The supporting C ABI repair preserves signed 64-bit Int arithmetic, parameters,
returns and heap Result payloads, including inclusive MAX ranges and MIN/-1.
Its legacy packed Option and other recorded C limitations remain separate.
The previous 1006-test checkpoint also passes C/Rust/docs gates on Linux arm64
with Rust 1.75; this does not establish Linux amd64, current performance or default
migration readiness. Full checker workflow parity is still being integrated.

## Editor control and collection syntax — 2026-09-06

The pinned editor grammar covers for/with, ranges, map literals and record updates,
condition matches, defer and loop control. Native and WASM parsers agree across
69 valid Rust-checked sources, 20 malformed inputs and 22 incremental edits.
The scanner preserves post-dedent expression boundaries and rejects indentation
beyond 1 MiB. Fresh native caches prevent results from another grammar snapshot.

Three explicitly tracked malformed inline headers still absorb the following
declaration. Full recovery, remaining syntax and Zed packaging stay open; see
[the exact editor contract](../../editor/tree-sitter-fern/README.md).

## Complete file-text Results — 2026-09-06

File.read now rejects invalid UTF-8, interior NUL and content above 16 MiB before
publishing a String. Native write/append check buffered close failures before Ok;
text preflight happens before opening a target. The REPL keeps the same text
policy and its stricter storage budget, with the safe File-drop limitation stated
in [the IO contract](../FILE_TEXT_IO.md). Native fault injection and source programs
cover late failures, exact limits, complete output and preserved preflight targets.

## Source argument labels — 2026-09-06

Direct source calls resolve optional labels against original parameter interfaces,
including external pattern names, module aliases and function clauses. Arguments
execute once in written order; pipe inputs execute first. The formatter and source
signature presentation retain labels. [The label contract](../LABELED_CALLS.md)
records positional compatibility, erased callable interfaces and the remaining
mandatory-enforcement/navigation/editor work. The complete checkpoint passes
1037 Rust tests on macOS arm64 and 1038 on Linux arm64, plus four native labeled
programs and four atomic invalid-output checks on both platforms.

## Pinned Unicode decimal text — 2026-09-06

`String.is_decimal` uses the same generated Unicode 16 Nd table in both native
frontends and the REPL. It preserves full Bool ABI and once-only callback effects,
rejects oversized input through Rust deferred cleanup, and charges interactive
scan work before execution. [The classifier contract](../STRING_DECIMAL.md) records
provenance and the fixed text profile. Exhaustive native Unicode tests pass in
debug/release/sanitizer builds on macOS/Linux arm64. Full C and Rust gates pass on
both platforms (1047/1048 Rust tests); native checker CLI adaptation remains a
separate follow-on.

## Zed extension packaging — 2026-09-06

The extension registers a pinned grammar and discovers `fern-rs`, with literal
path/argument overrides. Staging builds a validated Preview2 component and the
portable canonical grammar, checks four executable queries and produces identical
archives from fresh builds. Fifteen package-boundary tests and five adapter tests
cover failures and command selection. Actual Zed1.18.0 sessions loaded the package
and exchanged diagnostics through both discovery and explicit-path modes in owned
temporary profiles. [The extension guide](../../editor/zed-fern/README.md) documents
separate tools, offline provisioning and the unpublished-grammar limitation.
This establishes local packaging and startup, not marketplace publication or full
language syntax parity.

## Formatter validation for CI — 2026-09-06

`fern-rs fmt --check source.fn` checks the same canonical formatter used by normal
`fmt`, without writing or creating temporary files. The flag also works after the
source path. Clean input exits0 silently; formatting drift exits1 with a path
diagnostic. Invalid input also exits1 and remains untouched. Six CLI regressions
cover both flag positions, literal paths, metadata, read-only symlink targets and
the transition from drift to clean after ordinary formatting. No backend/runtime
is needed. Recursive formatting and the C CLI check mode are separate work.

## Native checker argument parity — 2026-09-06

The native checker's negative-path handling now matches Python 3.14's decimal
prefix semantics, including supplementary Unicode digits and arbitrary suffixes.
All 66 workflow cases pass under both frontends; the earlier executable known-gap
case is closed. A native default launcher remains separate work.

## Required source argument labels — 2026-09-06

Direct calls now require labels for exact Bool and repeated identical finalized
declared parameter types. Aliases normalize before classification; distinct
generics and newtypes remain distinct. Required pipe positions use labeled holes.
Source fixtures and checker helpers retain their written evaluation order after
label insertion. Ten additional Rust regressions cover required interfaces and
hostile label metadata; the native gate now has five programs and eight atomic
rejections. Structural function values, lambdas, runtime APIs and constructors
keep positional interfaces. Label navigation and full editor parity remain open.

This checkpoint passes the full Rust and C gates on macOS arm64 and Linux arm64
with Rust 1.75: 1063 Rust tests on macOS and 1064 on Linux. Native checker workflow
parity remains green across all 66 cases under both frontends.

## Source label tools — 2026-09-06

Definition and hover on external argument labels now retain exact source parameter
identity across clauses, aliases, reexports and current dependency overlays. Declared
generic types remain generic in hover. Invalid current graphs publish no label
facts; the member-recovery path also retains required-label checks. The checkpoint
adds ten Rust tests, bringing coverage to 1073 on macOS and 1074 on Linux.

The full editor gate verifies 80 valid sources, 27 malformed cases and 27 incremental
edits across native and WASM parsers, four executable queries and strict Rust checks.
Three named malformed-header recovery gaps and incomplete-call label completion
remain open. The Zed revision is updated separately after committing the grammar.

## Malformed-header editor recovery — 2026-09-06

All three named inline for/with recovery gaps are closed, preserving the original
malformed sources and genuine error/missing-token ranges. The expanded pinned gate
passes 85 valid sources, all 33 malformed cases retaining the following declaration,
and 30 incremental edits. Public, CRLF, blank-line, keyword-prefix and column-zero
lambda boundaries are covered; scanner state and serialization limits are unchanged.

## Matching Zed grammar package — 2026-09-06

The extension now pins verified grammar revision
`6d4efbb2f14a73be872f7c8e94c5ac31e54afcb9`. A stale-revision regression first failed
on the new label query; the updated package passes exact label captures, all
85/33/30 native/WASM corpus cases, hostile-package checks and two identical archives.
Actual Zed startup opens a labeled-call fixture with clean diagnostics through both
Rust-server discovery and explicit-path configuration in isolated temporary profiles.
The package remains local; its source revision has not been published remotely.

## Frontend completion scope — 2026-09-06

Decision96 concentrates new source-language work in Rust. The C frontend remains
the current shipping default until the explicit migration gates pass; afterward
it is retained as a bootstrap/reference executable with documented legacy behavior.
Its tested native ABI symbols remain available. This replaces duplicated C-source
feature expansion with a verified command/API migration, while keeping typed JSON
codecs, standard-library completion and executable language semantics open.

Additional x86-64 Linux checks used QEMU with Rust 1.75 and Clang 14: 32 core, five
label, 13 newtype and 27 union native programs passed, plus their 50 invalid inputs.
The full Cargo run stalled and reached its external timeout, so this is limited
emulated execution evidence, not a passing full platform or performance gate.

## Source-label completion — 2026-09-06

The Rust LSP suggests current source parameter names in closed and EOF-open calls.
Canonical module resolution, privacy, current overlays and lexical shadowing select
the interface; supplied arguments and original pipe placeholders reserve positions.
Suggestions provide exact UTF-16 edits without claiming checked types or label
requirements. Recovery stays private to parser/source indexing and does not create
executable hole IR. Unsupported malformed sources fail recovery; ordinary lexical
completion remains available when no source label matches.

Twenty new protocol regressions bring the Rust checkpoint to 1093 tests on macOS
and 1094 on Linux arm64. The full Rust, C and documentation gates verify integration.

## Numeric editor literals — 2026-09-06

The editor now parses the compiler's binary/octal/hexadecimal literal spellings,
integer separators and exponent-only Floats. Eight new compiler-checked sources
assert exact numeric tokens, including patterns, signed minimum Int and ranges;
three incremental edits change radix, exponent and range form. The pinned native
and WASM gate passes 93 valid sources, all 33 unchanged malformed recovery cases
and 33 edits. Generated artifacts remain reproducible; full syntax parity stays open.

The local Zed extension pins this grammar revision. Its package passes all 93/33/33
syntax and edit cases, reproducible packaging and hostile-input checks. Isolated
actual Zed runs verify initialization, opening a source and clean diagnostics through
both the configured override and package discovery. This is local verification;
the grammar and extension have not been published.

## Project formatting — 2026-09-06

`fern-rs fmt src/` formats a bounded directory of Fern sources. The matching
`fern-rs fmt --check src/` reports every dirty path in sorted order without writing;
the flag also works after the directory. Hidden/build/dependency directories and
child symlinks are skipped. Explicit file and root-directory symlinks remain supported.

Every source is validated before any file is staged. All changed files are staged
with their original permissions before the first atomic per-file rename. Syntax,
input-budget and staging failures leave originals untouched. Publication is not a
project transaction: a later rename failure may follow earlier completed changes.
Discovery limits are 256 files, 8192 entries, 32 levels and 4096-byte paths; input is
at most 1 MiB per file and 8 MiB total, with a separate 8 MiB formatted-output cap.
Twelve new regressions cover these boundaries, exclusions, sorted diagnostics,
read-only checks and staging cleanup, alongside all six existing file-check tests.

## Editor formatting — 2026-09-06

The Rust LSP advertises full-document formatting and returns canonical edits for
the latest accepted unsaved buffer. Clean source returns an empty edit list;
malformed syntax returns a formatting error. Formatting needs no checked imports
or backend and does not change disk files, buffers, versions or diagnostics.
The editor applies the returned UTF-16 replacement. Fern keeps its canonical
four-space style even when the client requests different whitespace preferences.
Eight protocol/executable regressions cover these rules, non-BMP text, CRLF, stale
changes, malformed options and lifecycle errors. Range/on-type formatting, rename
and code actions remain open.

The complete Rust gates pass 1113 tests on macOS and 1114 on Linux arm64. Isolated
actual Zed startup succeeds through both discovery and an explicit compiler path;
its recorded initialize responses receive the formatting capability. The format
request itself is verified through the library and executable protocol tests.


## Concrete typed JSON codecs (J4) — 2026-09-06

Decision98 adds `json.encode(value)` and `json.decode(text, TargetType)` with
explicit `derive(Json)` on acyclic records. Both return ordinary Results in native
programs and the REPL. The type-only target supports module aliases and input
pipes; no executable type expression or symbolic plan enters public IR. Concrete
generic records, supported containers and exact primitive adapters share strict
field/nullability rules and JSON Pointer errors.

All codec phases and children share bounded work, allocation and path accounting.
Public plan validation includes inactive entries and functions with one aggregate
allowance, while QBE deduplicates immutable descriptor tables and bounds output.
The authored `runtime/fern_json_codec.inc` is included in runtime source inventories;
new source syntax remains Rust-only and legacy C JSON symbols remain unchanged.
See [the complete J4 contract](../JSON_TYPED_CODECS.md).

Integration passes 1,153 Rust tests on macOS arm64 and 1,154 on Linux arm64 with
Rust 1.75, native source/atomic-output gates, old dynamic JSON oracles and new
typed ABI/path/shared-budget tests in debug, release and ASan/UBSan. The first
Linux attempt hit an existing timeout-test assertion; its isolated rerun and
the final complete gate passed. An intervening run required copying the new
native fixture directory into the owned container.

The editor grammar now checks 98 valid sources against the integrated compiler,
recovers all 33 existing malformed cases plus a trailing-derive-comma regression,
and verifies 36 incremental edits in native and WASM parsers. Record derivation
names have exact syntax/query tests; trailing commas reject consistently with Rust.
Matching Zed package pinning follows the grammar commit; no remote publication
is implied. Recursive/newtype codecs, generic constraints, sum/union wire formats
and general/custom traits remain J5–J7 work.


## Matching typed-JSON Zed package — 2026-09-06

The package pins `20f3cb053a35a1321bb0c3932a3d3c842692e74d`. The stale grammar
first failed to compile the new derive highlight query. The matching revision
passes two byte-identical archives, hostile component/query checks and the
98/34/36 native/WASM corpus. Zed 1.18.0 starts the Rust server and opens derived
record/codec source with clean diagnostics through both discovery and explicit
binary configuration in isolated profiles. Toolchain metadata records the same
grammar revision. The package remains local; remote publication is not implied.


## Regular recursive typed JSON (J5a) — 2026-09-06

Derived trees, mutually recursive records with optional bases, recursive Maps and
finite generic permutations now use exact instantiated type identities in indexed
codec graphs. Strict cycles without a finite value and expanding generic schemas
reject before execution; cached back edges do not skip later unsupported fields.
The public plan validator independently checks every entry and edge, including
inactive entries, under one aggregate budget. A linear finite-value proof matches
an independent oracle for all 65,536 four-node strict graphs.

The native descriptor ABI is unchanged. Repeated visits retain the same depth,
work and path accounting: a hostile cyclic native value reports code 4 at the
same depth and 704-byte path as an equivalent deep chain. Four new native/REPL
programs extend the typed-codec corpus to eight programs plus fourteen atomic
invalid inputs. Newtype derivations and generic constraints remain separate
Decision101 stages; sum/union wire formats and general traits remain open.

Integrated verification passes 1,165 Rust tests on macOS and 1,166 on Linux, full
C compatibility gates on both platforms, native/REPL codec tests, runtime
debug/release/sanitizer profiles, and documentation checks. Linux's first parallel
Cargo run reproduced a preexisting freshly-written shell-fixture race: native
capture returned `Text file busy (os error 26)` before the timeout assertion.
The unchanged-source full Linux gate passes with `RUST_TEST_THREADS=1`; a separate
fixture reliability fix remains tracked. This serialization is not needed by the
recursive JSON tests themselves.


## Explicitly derived newtype JSON (J5b) — 2026-09-06

`newtype UserId derive(Json) = UserId(Int)` opts a distinct nominal type into
transparent JSON conversion. Derivation metadata survives imports, type/value
name collisions, source documentation and canonical formatting. Generic wrappers
and finite recursive wrappers over containers retain exact nominal identity.
The compiler and public plan validator reject wrong storage, wrong payload types,
forged optional-field markers and unsupported derivations.

Native descriptor tag 11 follows one child with another depth/work step and no
wrapper allocation. Native tests compare actual GC allocated bytes as well as
logical budgets against raw Int/Float adapters, and preserve Int MIN/MAX, negative
zero and subnormal bits. A newtype wrapping Option accepts explicit null but is
still a required record field; only actual Option fields default to None when
missing. Option of a nullable newtype remains ambiguous and rejects.

The typed native/REPL corpus now has ten programs and eighteen atomic invalid
inputs, plus five native runtime fixtures across debug/release/sanitizers.
Conditional generic codec-function requirements remain the next Decision101
stage; sum/union formats and general/custom traits remain open.

Editor support now verifies the newtype derive header in native and WASM parsers,
including public/generic declarations, highlight and outline fields, the 32-trait
limit, malformed headers and incremental break/repair edits. All previous
98 valid, 34 malformed and 36 edit cases remain unchanged; the expanded corpus is
102/38/41. Both regenerated WASM modules are 288,352 bytes with SHA256
`8877901ef6c339bde4211eb55d97f7397071b5c20a16b4e7220d3f19df866ed0`.
Matching Zed package pinning follows the grammar commit.

Integrated macOS verification passes 1,174 Rust tests; Linux passes 1,175 with
the previously documented capture-fixture serialization. Full C compatibility
gates pass on both platforms. The shared editor gate confirms both newtype
queries and the original record-derive query regression, with no existing source
case removed. Newtype allocation and full-width payload checks pass in all three
native runtime profiles.


## Concurrent native capture fixtures — 2026-09-06

The intermittent Linux `Text file busy` failure was reproduced on Rust 1.75 by
eight workers repeatedly creating and executing shell fixtures. The test seam
now passes a literal command to the existing capture mechanism and invokes the
stable `/bin/sh` interpreter for fixed test bodies. Public executable capture
still takes a path; application execution does not gain a shell fallback.

Timeout, output limits and inherited-output descendant tests remain intact.
A new 8×20 concurrent regression verifies independent exact output and nonzero
status. Twenty repeated Linux parallel runs pass 3,200 adversarial invocations
without retries or serialization, alongside macOS checks and Clippy. Full platform
gates now use normal parallel Cargo execution. The separate existing lifecycle
audit—retaining child identity until process-group cleanup—remains Decision102
work and is not claimed fixed by this fixture change.


## Native quality-checker default (Decision93) — 2026-09-06

The default style, lenient and pre-commit recipes now launch the compiled Fern
checker through a content-validated C-bootstrap cache. Full `mise run check` keeps
its independent Python integration cases, while ordinary style checks require
neither Python nor Cargo. Seven real-Just dispatch regressions first failed when
Python tools were disabled, then passed with exact native arguments, streams
and failure propagation. CI retains source-compiled reference checks and also
verifies the cached launcher and its infrastructure suite.

Both platform deliveries passed supervisor/controller/descriptor/inventory
matrices in debug/release/sanitizers, 66 independent workflow cases, immutable
cache and external dependency checks, concurrency, interrupted cold starts,
one-retry freshness and bounded ownership cleanup. Clang14 configuration
injection initially reproduced a portability gap; the final private empty config
and compiler-scoped default suppression pass all three implicit config locations
on Linux14 and Mac16. Actual current-source cold/warm and no-Python checks pass.
The [launcher contract](NATIVE_STYLE_CHECKER.md) specifies exact bounds and the
initial helper, escaped-group and filesystem-freshness limitations.

The implementation changes the quality-checker entry point. The language's
default compiler remains C pending the separate Decision96 command migration.

Integration also reproduced empty `XDG_CACHE_HOME` selecting `/fern-style` rather
than the documented HOME fallback. The launcher and cleaner now share unset/empty
fallback semantics. An actual native regression populates an isolated HOME cache
with XDG unset, reuses it with XDG empty, then verifies cleanup selects that same
cache. No user cache or parent directory permissions are modified.


## Conditional generic JSON requirements (J5c) — 2026-09-06

Generic codec wrappers now infer and retain Json, non-null payload and exact
String-key requirements through explicit signatures, recursive calls and source
function values. Concrete specialization validates the exact static decoder type
and produces the existing full-width plan. Phantom type parameters introduce no
stored codec requirement; actual function and Result payloads remain unsupported.

Private codec templates retain input effects during generic checking. Their
constructor is inaccessible to external IR callers, relationships and aggregate
work are validated, and ordinary IR/QBE/REPL publication rejects them. Runtime
representation, numeric rules, error paths and operation budgets remain unchanged.
A 210-case structural oracle compares conditional discharge to concrete eligibility.
Sum/union formats and general/custom traits remain subsequent stages.

Integrated macOS and Linux gates pass 1,196 and 1,197 Rust tests respectively,
with normal parallel test execution, thirteen typed native programs and twenty-two
atomic invalid cases, the shared REPL corpus, and native runtime debug/release/
sanitizer checks. Prior dynamic JSON and native compiler suites remain green.


## Reachable Result handling (Decision95) — 2026-09-06

The Rust checker now proves handling and complete return provenance before
publishing typed IR, source facts or REPL state. Borrowing helpers remain valid;
their callers retain outstanding duties. Boolean path predicates preserve alias
correlations, nested Result layers and early-exit/defer behavior. Full collection
families differ from partial searches and projections, and fold transitions must
retain or handle prior accumulator duties. Source/generic/callable summaries carry
actual effects rather than relying on function type equality.

The proof shares its bounded work through generic codec-template validation and
concrete publication. Real codec inputs borrow; their fresh Result outputs remain
accountable. Phantom metadata is separate from actual stored fields. Private
recovery operations cannot erase surrounding effects or publish executable holes.

The pre-JSON corpus retained all 234 accepted programs unchanged. Two JSON callback
fixtures are explicitly migrated from per-item `?` to exhaustive per-item handling:
the old pattern could abandon already-produced later Results on Err. Successful
stdout is unchanged, and a paired regression preserves the original rejection.
The corrected current corpus retains all 247 accepted programs. Recursive nominal
stored-Result trees and broader structural fixed points remain separate completion
work; see [the exact contract](../RESULT_HANDLING.md).


Integrated macOS and Linux gates pass 1,302 and 1,303 Rust tests respectively,
including native compiler programs, runtime sanitizer profiles, fuzz smoke tests,
full C checks, source/native checker diagnostic and workflow parity, and
documentation checks.


## Shared Apple arm64 backend register correction — 2026-09-06

The vendored QBE emitter used Apple's reserved x18 register for integer swaps and
constant spills. The Apple allocator now reserves x17 for those scratch paths,
while generic arm64/Linux output remains byte-identical. Both compiler frontends
use this backend. New assembly and native-result tests cover integer/Float swaps,
forty live values across calls, constant spills and a permitted caller x17 clobber.
A controlled scalar x18 corruption probe demonstrates the old defect on Linux.

The investigation began with a crashed native style scan. Its tuple-return path
uses x18, but the sample does not prove the precise trigger. Twenty scans rebuilt
with the correction pass with identical output. No GC, source-recursion, launcher
or timeout workaround is introduced; this evidence does not establish that every
possible native crash has been eliminated.


## Recursive Result handlers (Decision95 R1–R2) — 2026-09-06

Direct recursive nominal handlers prove each current-node Result and strict child
subtree independently. Distinct siblings retain distinct duties; equal layouts,
reconstructed roots and nondecreasing calls grant no induction credit. Complete
List/Map child traversals, exact recursive callbacks, Option/newtype edges and
per-child deferred cleanup preserve actual descent through private proof contexts.
Dynamic map deletion may empty the map and cannot reuse its prior nonempty fact.

Forty-two source cases independently pass ordinary parsing/type checking before
their expected handling acceptance/rejection is verified. The existing accepted
corpus remains 247/247. Mutual structural induction, recursive builders and richer
recursive callable equations remain open; see [the contract](../RESULT_HANDLING.md).


## Retained native unit/doc test supervision (Decision102) — 2026-09-06

The native test helper retains the direct child through group cleanup, then reaps
it exactly once. Safe Rust validates complete bounded binary frames and keeps the
stdin liveness guard open through wait. There are no detached Rust output readers
or numerical test-group signals. Ordinary exit125 stays distinct from helper
transport failure. Debug and Rust release builds include the helper component.

Native lifecycle/protocol/ownership/cancellation/concurrency checks pass in debug,
release and sanitizer profiles on macOS/Linux, including exact output caps and
malformed transport. Existing unit/doc source tests retain their output and exit
contracts. The Linux suite also verifies that escaped pipe holders cannot leave
Rust reader threads behind. See [component requirements](../../compiler-rs/README.md).


## Tagged JSON sums (Decision103 J6a) — 2026-09-06

Explicitly derived sums use stable source constructor names in strict tag/fields
envelopes. Finite-value proof supports constructor alternatives without hiding
invalid stored fields. Native and interactive execution charge real envelope
nodes, payloads and error paths against the same existing allowances.

The integrated macOS/Linux gates pass 1,354/1,356 Rust tests, eighteen typed native
JSON programs and twenty-eight atomic invalid programs. Seven runtime ABI fixtures
pass debug/release/sanitizer profiles. Full C checks, the native checker workflow,
compiler fuzz smoke and documentation gates also pass. Union codecs and general
traits remain separate; see [the wire contract](../JSON_TYPED_CODECS.md).


## Zed newtype grammar pin — 2026-09-06

The local Zed package now pins grammar revision
5b02ec61136eac8f07afc8a09817f702542fcc05. Exact package inputs pass the locked
extension tests, 102 valid/38 malformed/41 incremental grammar cases, native/WASM
queries and reproducible hostile-path archive checks. Actual Zed1.18.0 startup
passes in compiler-override and discovery modes with derived-newtype codec source.
This is local package/LSP verification, not remote publication or GUI formatting.


## Disjoint JSON union codecs (Decision103 J6b) — 2026-09-06

Canonical concrete union plans now select a member from bounded JSON kind,
strict record keys, tuple length or source sum tags before decoding it exactly
once. Ambiguous/unsupported shapes reject; no trial payload allocation or
first-success decoder changes the selected interpretation. Code14 reports no
unique member at the current parent path. Original numeric and selected-envelope
errors remain precise. Actual Option field status differs from nullable newtypes.

Whole-union generic requirements retain exact independent evidence, including
String-only Map keys. Non-union signature positions resolve before canonical
union equations, so generic duplicate collapse does not require a witness.
Direct compound decoder targets still use a named alias. Deeper payload/value
proof and general/custom traits remain separate; see [the contract](../JSON_TYPED_CODECS.md).

The integrated macOS/Linux gates pass 1,397/1,399 Rust tests, 36 typed native
programs, 33 atomic invalid programs and eight runtime ABI fixtures in each of
three profiles. Existing dynamic JSON, native compiler, full C, native checker
workflow, fuzz smoke and documentation checks pass. Independent exhaustive key-set
and finite-graph oracles retain their separate coverage. Golden stdout files keep
intentional final empty lines, including the empty root JSON Pointer.
