# Fern Rust compiler

The Rust compiler is the default `fern` command produced by this checkout's
build, release and install tasks. `fern-c` retains the C frontend for explicit
reference tests. QBE is the default native backend; Cranelift is an opt-in feature.
The native C runtime, QBE adapter, test supervisor and editor grammar remain
separate components. This compiler default does not imply completion of every
planned language feature or release acceptance gate.

The compiler uses safe Rust and pinned dependencies, including `rustyline` for
interactive terminal editing. Optional Cranelift dependencies are feature-gated;
`compiler-rs/Cargo.lock` records the complete dependency graph.

## Run it

Install the [native build dependencies](../BUILD.md), then run `mise install`
from the repository root. Fern uses **nightly-2026-09-06** with Cargo, rustfmt,
Clippy and rust-src; `rust-toolchain.toml` selects the same pin for direct Cargo
commands. The numeric Cargo requirement `1.100` is a minimum version check,
not a stable MSRV promise. The Rust compiler uses edition 2021. Decision112
permits pinned production dependencies for the optional Cranelift backend.
From the repository root:

```sh
mise run debug
./bin/fern check compiler-rs/tests/corpus/hello.fn
./bin/fern emit compiler-rs/tests/corpus/hello.fn
./bin/fern build compiler-rs/tests/corpus/hello.fn -o hello-rs
./hello-rs
./bin/fern run compiler-rs/tests/corpus/string_function.fn
./bin/fern run compiler-rs/tests/collections/propagation.fn
./bin/fern run compiler-rs/tests/types/project/main.fn
./bin/fern fmt source.fn
mise run rust-check
```

## Try the Cranelift backend

Build the experimental feature into a separate executable:

```sh
mise run rust-cranelift-build
./bin/fern-rs-cranelift build --backend=cranelift examples/tiny_cli.fn -o hello-cranelift
./hello-cranelift
./bin/fern-rs-cranelift run --backend=cranelift examples/tiny_cli.fn
mise run rust-cranelift-check
```

QBE remains the default, including in this feature-enabled executable. The
`--backend=cranelift` option applies only to `build` and `run`; `emit` continues
to expose QBE IL. An ordinary build without the Cargo feature reports an explicit
error if Cranelift is requested. Native unit/documentation tests and preview
packaging retain their existing QBE workflow.

The selected backend writes native objects in Rust without invoking QBE or an
assembler. A host linker, the existing C runtime and its native libraries remain
required. This does not make Fern's runtime or repository entirely Rust-based.
The mise build also prepares the QBE and test helpers so the same executable can
run the reference workflow. It copies the feature-enabled compiler to
`bin/fern-rs-cranelift`, leaving the default `bin/fern` and its development
alias `bin/fern-rs` in place.

Cranelift dependencies are pinned to 0.135.1 with transitive versions in
`compiler-rs/Cargo.lock`. See [the backend assessment](../docs/BACKEND_REASSESSMENT.md)
for the supported-update policy, acceptance gates and remaining debugger and
performance work. Integration is experimental; do not interpret the older scalar
benchmark as an end-to-end result for this compiler.

## Command behavior

Global `--quiet`, `--verbose` and `--color=auto|always|never` controls work before
or after the command. Quiet suppresses compiler/test summaries and prompts while
preserving errors and program output. Verbose reports the command on stderr;
color never changes emitted IR, documents or protocol data. Arguments after
`run source.fn --` remain literal, including global-looking flags. Explicit help
and version remain visible in quiet mode; `-v` aliases `--version`. Missing a
command returns usage failure (status 1).

Frontend compilation needs no C runtime build. The complete Cargo test suite
compiles a small native supervisor fixture with the host C compiler:

```sh
cargo test --locked --offline --manifest-path compiler-rs/Cargo.toml
cargo run --locked --offline --manifest-path compiler-rs/Cargo.toml -- check examples/tiny_cli.fn
```

`check` and `emit` use only Rust. The default QBE `build` and `run` paths require
`fern-qbe`, `libfern_runtime.a`, a host C compiler, and the existing runtime's native
libraries. Selecting Cranelift removes the QBE and assembler execution stages;
runtime discovery and final native linking use the same contract.
Installed/default binaries locate the helper and archive beside the actual
compiler executable. The sibling `fern-package.json` marker disables implicit
checkout fallback. Direct Cargo development binaries without a package marker
may also use the development checkout. [Separate preview bundles](../docs/RUST_PREVIEW_PACKAGING.md)
use the same component-discovery boundary. `FERN_QBE` and `FERN_RUNTIME_LIB` override their paths; `CC`
selects a single compiler executable (not a shell command). `run source.fn -- args` forwards
literal arguments, available through `System.arg`, `System.args`, and `System.args_count`.

Native documentation/unit tests additionally require `fern-test-supervisor`, built by
`mise run debug` and `mise run release` (also the `rust-build` compatibility task). Discovery uses
`FERN_TEST_SUPERVISOR`, then an executable sibling; development binaries without
a package marker may also use the development `bin` directory.
An absent or incompatible helper fails explicitly; there is no post-reap group-kill
fallback. The helper is a trusted executable component, not an untrusted protocol
server. Rust takes the stdin liveness guard before waiting, validates the bounded
version-one binary frame and EOF, and uses no detached readers or numerical PID signals.
Each output stream is limited to 256 KiB. Positive deadlines up to 60 seconds round
up to milliseconds and initiate cleanup; kernel reaping can extend elapsed time.
The native helper has another 1 second to publish after cleanup. Escaped process
groups and deliberate same-user namespace races are outside containment. Native
exit125 remains a test result, separate from helper transport failure.

## Supported language

- Top-level functions with optional private annotations, generic signature inference and forward/recursive calls.
  Public function signatures require return annotations. Parameterless `main` returns
  `Int`, `Unit`, or `Result((), E)` with a concrete error type; omitted `main`
  return type means `Unit`. Unresolved recursive return types require an annotation.
- Signed 64-bit `Int`, IEEE double `Float`, `Bool`, UTF-8 `String`, `Unit`/`()`, and recursively nested
  concrete `List(T)`, `Option(T)`, and `Result(T, E)` types.
- Custom sum and record declarations, concrete generic type applications, record fields,
  and generic functions specialized for their concrete call types.
- Transparent type aliases, including generic aliases: `type UserId = Int` and
  `type Maybe(a) = Option(a)`. Aliases preserve their target equality/layout and
  add no constructors; cycles, invalid arities and excessive expansion are errors.
  Formatting, docs and navigation retain the declared alias spelling.
- Distinct newtypes, including generic payloads: `newtype UserId = UserId(Int)` and
  `newtype Wrapper(a) = Packed(a)`. Construction, `.0` and constructor patterns
  preserve nominal identity and add no native wrapper allocation. Full-width Float
  payloads work through functions, collections and captures; recursive payloads
  behind existing heap containers are supported. See [newtype semantics](../docs/NEWTYPES.md).
- Module declarations, public functions/types, qualified/aliased/selected imports,
  and public reexports from `module.fn` or `module/mod.fn`.
- Immutable `let` with inferred or annotated type, lexical scopes, and shadowing.
- Structural tuple types/literals, nested tuple/list patterns, suffix binding with `..rest`, and `.0` field access.
- Standard and placeholder pipes evaluate their input exactly once before other arguments.
- Integer/Float arithmetic, powers and comparisons, Int bitwise operations, string addition/equality, boolean operators
  with short-circuit evaluation, and unary `-`/`not`.
- Inline and indented `if`/`else`, including value-producing branches. An `if`
  without `else` has type `Unit`.
- Early returns, postfix conditionals, condition-only matches and `let-else` unwrapping.
- Sequential `with` Result bindings with typed handlers or implicit error propagation.
- List/Map/range `for` iteration, list enumeration and scoped `break`/`continue`.
- Dynamic `defer` cleanup runs at function exit, including early return and `?` propagation.
- `print`, `println`, `String.concat`, `String.eq`, and `String.len`.
- Immutable list literals, `List.len/get/head/tail/is_empty/push/reverse/concat`,
  and scalar `List.contains` (Int, Float, Bool, or String elements).
- `Some`, `None`, `Ok`, and `Err`, with payload types inferred from bindings,
  function signatures, calls, and branches. Empty lists also use this context.
- Exhaustive `match` on scalar literals, Bool, lists, nominal types, Option, or Result, with wildcard
  and name catchalls, recursively nested constructor patterns, guards, and scoped multiline arms.
  Guarded arms do not establish exhaustiveness.
- Postfix `?` unwraps Ok or immediately returns Err from a Result-returning helper;
  the enclosing error type must agree. Result expressions cannot be silently
  discarded. Result-returning main may propagate errors with `?`.
- `Option.is_some/is_none/unwrap_or` and `Result.is_ok/is_err/unwrap_or`.
- Multiline lists, calls, and type arguments with checked delimiters.
- Nested string interpolation with Int/Float/Bool/String expressions and escaped literal braces.
- Immutable Map values with Int/Bool/String keys, ordered inspection and optional lookup;
  record updates that evaluate their base and fields once.
- First-class named/anonymous functions, contextual callback inference, escaping captures
  and typed higher-order List/Option/Result operations.
- Line and nested block comments, triple-quoted strings, declaration documentation,
  Unicode identifiers, validated base-prefixed integers and source-located diagnostics.
- Audited runtime bindings for files, strings, regex, arguments/processes, SQLite,
  HTTP clients, explicit mailboxes and terminal UI. Opaque annotations are qualified
  (`Tui.Panel`, `Tui.Tree`, etc.), keeping ordinary user-defined names available.

Unsupported syntax produces diagnostics. [Bounded native actors](../docs/RUST_ACTORS.md)
now execute; generalized suspension, typed supervision and actor REPL/FernSim
parity remain open. Source argument labels and private signature generalization
are supported. See [migration progress](../docs/RUST_MIGRATION.md) for acceptance
evidence and [language parity](../docs/LANGUAGE_PARITY.md) for the audited
executable compatibility surface.

`fmt source.fn` and `fmt src/` format supported syntax in place, preserve comments,
and verify that each complete syntax tree remains equivalent. Directory formatting
validates all selected sources and stages all changed files before any replacement;
publication is atomic per file. Hidden/build/dependency directories and child
symlinks are skipped. `fmt --check src/` reports all dirty files without writing. The Rust LSP also
exposes canonical full-document formatting over the current unsaved buffer; the
editor applies the returned edits.
`textDocument/prepareRename` and `textDocument/rename` support current-document
local bindings, private functions and private aliases. Renames use source binding
identities, recheck the edited source with unsaved dependency buffers, and reject
capture even when both bindings have the same type. Comments, literal string
contents, unrelated shadows and field selectors remain untouched. The returned
`WorkspaceEdit` includes the accepted document version and UTF-16 ranges; the
server never changes files or buffers itself. Rename is advertised only when the
client enables `workspace.workspaceEdit.documentChanges`; prepareRename also
requires `textDocument.rename.prepareSupport`. Literal formatting actions also
require `textDocument.codeAction.codeActionLiteralSupport`. Missing, false or
malformed support disables the corresponding feature, and unsupported requests
return a clear error; diagnostics, navigation and ordinary formatting stay available.
Public declarations, public default
parameter labels, imported declarations, members and ambiguous label references
require workspace refactoring and are rejected. Requests require valid checked
source and are bounded to 512 matching old/new identifiers, a 2 MiB source graph,
and 32 MiB of reference indexing work. New names have a 128-byte limit.
`textDocument/codeAction` offers a versioned canonical formatting edit with kind
`source.fixAll.fern`, honors `context.only`, and returns no action for clean or
unparseable source. It does not advertise diagnostic repairs that it cannot apply.
Comments inside multiline arguments may move adjacent to their statement. Invalid
source leaves the selected files untouched. See the [directory formatting limits](../DECISIONS.md#99-format-source-directories-after-complete-input-validation).
Every generic body is checked before specialization, including unused definitions;
concrete instantiation supplies a second validation boundary.

An expression such as `let empty = []` or `let missing = None` needs enough later
usage or an annotation to determine its payload type. For example,
`let empty: List(String) = []` is concrete. Unsupported compound equality is
rejected; it does not silently compare pointers. Invalid `List.get`/`List.head`
access reports a runtime error after deferred cleanup. Their signatures still
return direct values; recoverable collection indexing remains future work.

Strings inherit the existing C runtime: embedded NUL is rejected, and `String.len`
counts UTF-8 bytes. String repetition rejects results larger than 16 MiB before
allocation; empty strings and nonpositive counts produce an empty string immediately.
String.slice uses clamped byte offsets and rejects endpoints inside a Unicode scalar.
Splitting on an empty delimiter returns complete Unicode scalars. Integer domain
faults are reported after cleanup, as described below. A source
file is limited to 1 MiB, 65,536 tokens, and a syntax nesting depth of 128. These
explicit source limits prevent unbounded parser recursion/allocation.

## List and tuple patterns

```fern
fn length(items: List(Int)) -> Int:
    match items:
        [] -> 0
        [_, ..rest] -> 1 + length(rest)

fn first_or_zero(items: List(Int)) -> Int:
    let [first, .._] = items else: return 0
    first
```

List patterns without rest require an exact length. A final `..rest` binds the
suffix; `.._` ignores it. Tuple suffixes remain tuples, including singleton tuples;
an empty tuple suffix is Unit. Ignoring a prefix or suffix that contains Result
values is rejected. Guards run after all structural checks and required bindings.

Ordinary let, for and with success bindings require irrefutable patterns. A
nonempty list prefix can fail, so use match or let-else. Zero-prefix `[..all]`
always matches a list and shares its original immutable value. Fixed tuple
suffix bindings are irrefutable when their prefix fits the tuple type.

Named list tails currently copy their suffix once the entire structural pattern
matches. Recursive decomposition can therefore take quadratic time; prefer for
or List.fold for large scans. Ignored tails allocate nothing. Interactive tail
copies share the evaluation work budget, and failed matches publish no bindings.
The compiler bounds both source pattern nesting and expanded coverage analysis.
Multiline match, if, for, with and callback expressions also work inside call
arguments, list elements and tuple elements, including inline separators/closers.
Result handling tracks structural obligations through aliases, projections,
branches, calls and supported recursive builders. Inspecting collection length
or other metadata does not discharge contained Result values, and handling an
outer Result does not automatically handle nested payloads. Retaining aggregate
Result obligations does not prove a recursive output collection is nonempty;
uncertain flattening paths remain rejected. Unsupported recursive or higher-order
relationships remain conservative diagnostics.

## Entry results and return inference

A private function with annotated parameters can omit its return annotation:
`fn twice(value: Int): value * 2`. Forward calls and recursive definitions with
sufficient type constraints infer concrete return types; unanchored recursive
cycles require an annotation. Public signatures remain explicit. Generic helpers
retain their declared type parameters and specialize independently per call.

`main -> Result((), E)` exits 0 for `Ok(())` and 1 for `Err`, after deferred cleanup.
An unhandled entry error currently prints `fern: main returned Err` to stderr;
rendering arbitrary error payloads awaits a general display protocol. A runtime
fault in the body or its cleanup takes precedence over the entry Result.

## Generic definition checking

Declared generic variables are rigid: `fn identity(x: a) -> a: 1` is an error even
if the function is never called. The checker also validates nested matches,
callbacks, return types and known Result obligations in unused definitions.

Generic operations retain their actual requirements. For example,
`fn square(x: a) -> a: x * x` accepts Int and Float instantiations, while
`fn describe(x: a) -> String: "value={x}"` supports scalar interpolation.
These requirements propagate through named calls, function values, closures and
recursive helpers. Map key restrictions also apply inside nominal field types.
No arbitrary Int instance is used to validate a generic body.

Intrinsic requirements are internal; public `where`/trait syntax remains future
work. Conditional restrictions on generic Result-bearing values are checked
through reusable function summaries and concrete instantiations. Proofs remain
bounded; the checker rejects relationships it cannot establish.

## Private signature inference

Private clauses can infer a shared input type from literals and constructors:

```fern
fn factorial(0) -> 1
fn factorial(n) -> n * factorial(n - 1)
```

This infers `Int -> Int` before considering callers. Nested list, tuple, Option,
Result and nominal patterns contribute constraints across the complete group.
An annotation in a later clause can anchor an earlier omission, including a
shared declared generic type. Tuple-rest patterns wait until another pattern or
annotation determines the complete tuple arity.

Private function bodies also contribute constraints, and completed functions are
generalized independently of callers:

```fern
fn identity(value) -> value
fn apply(action, value) -> action(value)
fn add(left, right) -> left + right
fn length([]) -> 0
fn length([_, ..tail]) -> 1 + length(tail)
```

`identity` and `length` can be used at different element types. `apply` infers the
relationship between a callback, its argument and its return. `add` retains an
addition requirement, so Int, Float and String calls work while Bool/List calls
are rejected. A literal can establish a concrete type: `fn double(x) -> x * 2`
accepts Int. Return-only collection shapes such as `fn empty() -> []` and
`fn missing() -> None` receive a fresh payload type at each use.

Recursive groups share constraints before publishing their signatures; completed
named functions instantiate freshly at each reference. Local function bindings
remain monomorphic. Explicit type variables stay rigid, public signatures remain
annotated, and unanchored recursive returns require annotations. Inferred
polymorphic recursion also requires a complete signature.

Later body evidence can resolve record fields and updates, exact tuple suffixes,
and List/Map/Range iteration. A projection such as `let y = value.field` can
precede the call or annotation that establishes the record type. Branch order
does not affect these constraints, and runtime evaluation order stays unchanged.
Unknown record types, iterable kinds and tuple arities still require an annotation;
the compiler does not choose them from callers.

## Function clauses and native recursion

Adjacent clauses support typed parameter patterns, guards and arrow bodies:

```fern
fn total([]: List(Int), acc: Int) -> acc
fn total([head, ..tail]: List(Int), acc: Int) -> total(tail, acc + head)
```

Clauses share parameter types and visibility, and any supplied return annotations
must agree. Public groups need a return annotation. Guards run in source order;
missing cases and unreachable clauses are errors. Patterns use the same rules as
`match`, including Result discard checks. Private parameter annotations can be
omitted when patterns and bodies establish a generalizable signature. Public parameters remain annotated. Put
whole-function `@doc` text before the first clause.

Native direct self calls in return position reuse the current stack frame when the
function has no owned `defer`. Arguments finish left-to-right before parameters
change; Float bits, pointers and the current fault context are retained. A function
with `defer` retains separate activations and cleanup for each call. Mutual and
indirect recursion still use ordinary calls. These are native optimizations;
interactive evaluation retains its explicit step/depth limits. List suffix patterns
still copy their tails, so tail-call elimination alone does not make repeated list
suffix traversal linear-time.

In the REPL, enter `:paste`, a complete clause group, then `:end` on its own line.
Blank lines inside paste mode are retained. Only a successful complete entry is
saved; EOF before `:end` discards the unfinished entry.

## Functions and callbacks

```fern
fn make_adder(base: Int) -> (Int) -> Int:
    (value: Int) -> base + value

fn main():
    let add = make_adder(40)
    println(add(2))
    let numbers = List.map([1, 2, 3], (value) -> value + 1)
    println(List.fold(numbers, 0, (total, value) -> total + value))
```

Both `(value) -> expression` and `fn(value) -> expression` support indented
bodies, including callbacks inside argument lists. Function types use
`(Int) -> Int` or `fn(Int) -> Int`. Context infers omitted lambda parameter types;
local function bindings remain monomorphic. Named generic and builtin/runtime
functions specialize from their concrete expected function types.

Captures evaluate once and survive their defining scope. Higher-order operations
preserve input order; `List.find`, `List.any` and `List.all` stop as soon as the
result is known. Empty collections and absent/error sums skip their callbacks.
Each callback has its own `?` propagation boundary. Capturing an already-produced
Result-bearing value currently gives an explicit diagnostic; delayed ownership
tracking is still required to lift that restriction. Returning a Result from a
function is supported. Function equality is not defined.

## Numeric values and literal text

Integers have signed 64-bit values with wrapping arithmetic in every build.
Decimal, `0x`, `0b` and `0o` literals accept checked digit separators and reject
out-of-range magnitudes. Power `**` is right-associative: `2 ** 3 ** 2` is 512.
Unary operators retain the existing precedence, so `-2 ** 2` is 4. Integer powers
require nonnegative exponents and define `0 ** 0` as 1. Float power uses IEEE/libm
behavior without implicit Int conversion.

Bitwise operators are `&&&`, `|||`, `^^^`, `~~~`, `<<<` and `>>>`. Shifts normalize
counts modulo 64; right shifts preserve the sign. Float list membership compares
values: signed zeros compare equal, while NaN never equals another NaN.

Integer division/remainder by zero and negative integer exponents produce a
runtime diagnostic. Called functions and callbacks unwind through deferred
cleanup; the first error is retained if cleanup also fails. Compiled main reports
one error on stderr and exits 1. The REPL reports the same error and retains prior
successful bindings. Runtime faults are distinct from explicitly handled Results.

Triple-quoted strings preserve newline and indentation bytes exactly and support
ordinary escapes and interpolation. Nested `/* ... */` comments are bounded and
must close. `@doc """..."""` attaches literal documentation to the immediately
following function or type. Formatting and documentation preserve that metadata;
Rust doc-test execution remains part of the tooling work. Non-ASCII identifier
spelling is retained exactly, without Unicode normalization.

## Iteration and grouped error handling

```fern
fn main():
    for (index, name) in ["Fern", "Rust"].enumerate():
        println("{index}: {name}")
    for number in 0..=3:
        continue if number == 1
        println(number)
```

Ranges are immutable `Range` values with Int endpoints. `start..end` excludes the
end; `start..=end` includes it. Reversed ranges are empty, and even an inclusive
range ending at the maximum Int does not overflow. A loop evaluates its iterable
once. Lists follow element order, maps yield `(key, value)` in insertion order,
and `List.enumerate(values)` or `values.enumerate()` yields `(index, value)`.
Loop patterns must match every element. Break and continue affect the nearest
loop in the same function; deferred cleanup still waits until function exit.

```fern
fn read_size(path: String) -> Int:
    with
        text <- File.read(path)
    do
        String.len(text)
    else
        Err(_) -> 0
```

With bindings execute sequentially and stop on the first error. Each error type
has a checked exhaustive handler; different steps may have different success
and error types. Handlers use `Err(pattern)` or `_` and see the outer scope,
while later steps and `do` see successful bindings. Guarded arms preserve source
order. Omitting `else` propagates errors under the same Result constraint as `?`.

## Returns and cleanup

```fern
fn describe(value: Option(Int)) -> String:
    defer println("finished")
    let Some(number) = value else:
        return "missing"
    return "negative" if number < 0
    "present"
```

A return exits its nearest function, including an anonymous function. Expressions
and arguments after an executed return do not run. A `let-else` failure branch
must leave the function; successful bindings remain available below the statement.
Condition-only `match:` arms test their Boolean conditions in order and require a
final `_` fallback.

Deferred expressions run in reverse registration order when the function exits.
A defer registered inside an `if` runs only if that branch executes, and waits
until the whole function exits. Immutable values are captured when registered;
the expression and its call arguments execute during cleanup. Return values are
evaluated first. Cleanup must return Unit and cannot use return or `?` to leave its
caller. Each called function and lambda has its own cleanup stack. The REPL also
attempts cleanup after evaluation faults, with a separate bounded work budget.

## Maps and record updates

```fern
fn main():
    let original = %{ "name": "Fern", "stage": "prototype" }
    let updated = Map.put(original, "stage", "development")
    println(Option.unwrap_or(Map.get(updated, "name"), "unknown"))
```

`Map.new`, `get`, `put`, `delete`, `len`, `is_empty`, `contains`, `keys` and
`values` retain semantic types. Empty maps require an annotation or later use
that fixes both key and value types. Values may include records, closures and
Results. Keys are Int, Bool or String; String keys compare by contents.

Updating a key leaves its insertion position unchanged. Duplicate literal keys
keep their last value, and all key/value expressions still execute in source
order. Deletion followed by reinsertion appends the key. Operations preserve
existing aliases. The initial implementation uses linear searches and copies on
updates; it is intended as a correct baseline for later hashing optimization.

Record updates use `%{ point | y: next_y(), x: next_x() }`. The base executes
first, followed by fields in written order. Unmentioned fields retain their
values. Unknown, duplicate or incorrectly typed fields produce diagnostics.

## Interactive and editor tools

`fern repl` evaluates expressions, successful `let` bindings and typed function
or type definitions. Submit indented blocks and continued calls with a blank line. `:help`, `:reset`
and `:quit` control the session. It checks typed IR and retains values without
replaying earlier effects. Core values, matching, tuples, generics, common string
and list APIs, and local file operations execute interactively. Unsupported native
APIs produce a source-facing diagnostic; use `run` for those programs. Evaluation,
input, allocations and value previews are bounded. Closures retain their original
compiled code across later entries; unique retained programs count toward the
interactive storage budget. Session errors preserve prior
bindings; already completed filesystem effects cannot be rolled back.

`fern lsp` serves JSON-RPC on standard input/output. It supports lifecycle,
UTF-16 diagnostics, full/incremental document changes and module-aware checking
against unsaved buffers. Imported errors retain their source URI; dependency edits
recheck callers and clear stale diagnostics. It currently reports the first error
per checked module graph. Go-to-definition follows source bindings, including
shadowed locals, clause parameters, captures and visible imported declarations.
Completion respects lexical scope and module visibility, replaces the current
identifier using UTF-16 ranges, and offers builtin prefixes such as `List.` even
when the surrounding source is incomplete. Comments and plain string contents
do not produce code suggestions.

Navigation rebuilds its bounded source index from current accepted buffers on
each request; it never falls back to stale locations from a previous valid edit.
It requires a parsable source graph but can work before type errors are fixed.
Completion returns at most 256 items and 1 MiB of output; retained editor symbol
names are capped at 8 MiB. Hover shows checked generic signatures, instantiated
uses, intrinsic requirements and literal documentation from the selected declaration.
Valid-source member completion shows record fields, tuple slots and supported
receiver methods with concrete types. Type annotations and value expressions use
separate namespaces even when a type and function share a name. These semantic
facts require the entire current program to check successfully; invalid edits do
not reuse old types. Completion also handles one unfinished record/tuple/member
selector inside a function with concrete parameter and return annotations; main
may omit its Unit return annotation. The receiver must have a concrete type before
the unfinished operation is checked. Unannotated/generic enclosing signatures or
unrelated errors retain lexical fallback. No partial proof becomes executable IR.
See [the recovery contract](../docs/EDITOR_RECOVERY.md). Local semantic rename
and versioned canonical formatting actions use the checked source and edit
contracts described above.

## Source documentation

`fern doc library.fn` writes Markdown to stdout. Use `--html -o docs.html`
for a standalone HTML page, or `-o docs.md` to save Markdown. Output replacement
is atomic; source files and their symlink/hardlink aliases cannot be overwritten.
`fern doc --help` describes the options.

Documentation uses the parser to retain function clause groups, guards, nested
signatures, type declarations and Unicode names. All declarations are included,
with their original visibility and annotations. Each literal @doc belongs to its
own declaration. Generation requires valid syntax, but no main, backend or code
execution. Use `--inferred` to add checked signatures as described below. HTML displays documentation as
escaped literal text; Markdown retains authored documentation markup.

Single files are bounded to 1 MiB and 4,096 declarations; single-file output is
limited to 8 MiB. Use the separate test command below to execute examples.

Directory documentation accepts `fern doc src --html -o docs.html` and produces
one standalone searchable page. Module paths stay distinct, even when files or
declarations have the same name. Search matches paths, signatures and doc text;
module links restore the full view before navigating. Markdown also accepts
directories. Generation parses source without executing code or requiring main.

Discovery includes `.fn` files recursively, excluding hidden entries and the
`target`, `build`, `bin`, `deps` and `node_modules` directories. Child symbolic
links are not followed. Limits are 256 files, 8,192 visited entries, directory
depth 32, 8 MiB combined source, 4,096 declarations and 16 MiB project output;
each source retains the parser's 1 MiB limit. Errors preserve the previous output,
and output aliases of any discovered source are rejected.

Use `fern doc library.fn --inferred` or `fern doc src --inferred --html`
to supplement original headers with resolved signatures and intrinsic requirements.
This checks each current module graph once, including private/generic bodies,
and never executes code. Imports and source anchors determine which declaration
owns each signature; generated generic names are rendered as ordinary variables.
Invalid graphs preserve previous output. Default source-only generation remains
available for syntactically valid code with unfinished type errors.

Checked generation protects both documented files and imported dependencies from
output replacement. It retains up to 1,024 cached source snapshots while each
module graph keeps the compiler's 128-file/8 MiB limits. Across roots, copied
snapshot bytes (copied once) plus loaded graph bytes are limited to 16 MiB, and at most 1,024
loaded source instances are checked. Metadata uses 16,384 type nodes and 1 MiB of
names per graph; each rendered signature/requirements block is at most 32 KiB.
Unrelated cached source contents are borrowed during graph loading. Library callers
supplying `FunctionInfo` must preserve facts from the same source snapshot;
identity and size checks cannot attest caller-edited semantic types.

`fern doc library.fn --open` generates HTML and opens it with the platform
opener (`open` on macOS, `xdg-open` on Linux). `--open` works with directories and
`--inferred` and implies `--html`. With `-o`, the destination is relative to the
current working directory; otherwise the retained artifact is `fern-docs.html`
there. Existing destination contents are replaced atomically after complete
successful generation, with the same source-alias protections as ordinary docs.
The canonical absolute artifact path is printed to stderr. Files remain available
after the command exits, including when the opener is missing or fails.

The opener receives one literal OS path without shell expansion and cannot read
terminal input. Its stdout/stderr are discarded; missing executables, unsuccessful
status and a ten-second launcher timeout produce a visible note even with
`--quiet`, while successful generation still exits 0. Only the directly owned
launcher is killed and reaped on timeout; the browser is not owned. A process stuck
in an uninterruptible kernel operation can delay reaping beyond that deadline.
No opener runs after parsing, checking, generation or publication fails. Other
platforms retain the HTML and report the unavailable opener. This Rust output
convention differs from the legacy Python script's `docs/generated` default;
no script command override or automatic browser/compiler installation is used.

## Executable documentation examples

`fern test --doc library.fn` executes each fenced `fern` block in literal @doc
metadata as a separate native test. A directory uses the same source discovery
rules as documentation generation; omitting the path selects the current directory.
The command executes user code and requires the native build dependencies.

Examples share their owning module's imports and private helpers, with independent
local bindings. A trailing `# =>` checks an ordinary Fern pattern against a complete
expression statement, evaluated once. For example, `add(2, 3) # => 5` checks a value
and `parse(text) # => Ok(_)` checks a successful Result. Multiline setup is allowed;
markers inside strings or block comments are ordinary text. Malformed expectations,
type errors, pattern mismatches and runtime failures produce a nonzero exit.

Original `main` functions remain callable, but only the example runs as the test
entry. Library checking validates all bodies without inventing an entry point.
Executable `check`, `build` and `run` still require main. No source files are changed.

Each example has a default 10-second runtime limit, configurable with
`--timeout 1` through `--timeout 60`, and 256 KiB limits for stdout and stderr.
Standard input is closed. On Unix, the test owns a private process group that is
terminated on completion, failure or timeout so descendants cannot hold output open.
There are at most 256 examples, 64 KiB per example and 1 MiB per source overlay.
Normal `fern test` also runs source-owned unit functions as described below.
Invoking System.exit fails both unit and documentation tests, so exit 0 cannot
bypass later expectations. Normal application compilation remains unchanged.

## Unit tests

`fern test [source.fn|directory]` runs zero-argument `test_` functions and
fenced documentation examples. Tests return Unit or Result(Unit,E); normal Unit
return and Ok pass, while Err, runtime faults and invalid test signatures fail.
Later tests continue. Generic tests and Boolean/integer results are rejected.
Imported functions run as tests only when their original source file is selected.

Eligibility uses the selected reusable source scheme during one ordinary library
check, before any concrete specialization can obscure generic parameters. Test
selection preserves resolved calls to the real main. Native timeout, stream and
process-group limits are shared with documentation execution; the combined suite
is bounded to 256 tests. `--doc` selects only documentation examples. See
[the test-runner contract](../docs/TEST_RUNNER.md) for limits and behavior.
Assertion libraries, benchmark, coverage and watch modes remain separate work.

## Architecture

```text
UTF-8 module graph → Rust lexer/parser → qualified source AST
            → name/type checking → typed IR (Type, FunctionId, LocalId)
            → shared typed machine IR (control flow, values, calls, data)
                → QBE text → fern-qbe process → assembly → object
                → optional Cranelift → native object
            → host linker + existing C runtime → executable
```

Semantic lowering consumes only typed IR. Both native backends consume the same
validated machine operations; Cranelift does not parse emitted QBE text. Lowering
never reconstructs a string's type from
its register width or variable spelling. Int and String both use QBE `l`, while
their semantic types stay distinct. Resolved function/local IDs separate source
names from generated symbols. An exported `fern_main` wrapper adapts Fern's
typed entry function to the runtime's C ABI.

Both Rust entry points forbid unsafe code. Owned enums and `Result` encode syntax
and fallible operations. The QBE adapter is a small C executable around the
vendored backend; its process boundary isolates global state and backend failure.
No C frontend executable or parser is invoked by Rust compilation. Fern's GC and
runtime semantics are unchanged. Lists and Results reuse the C runtime's existing
heap representations. Rust Options also use its heap-backed Result allocation,
tag, and payload helpers internally: Some is Ok and None is Err with unused zero
payload. This preserves full 64-bit integers and pointers. The C compiler's packed
Option ABI truncates payloads to 32 bits. Rust explicitly adapts the lossless byte
payload from `String.char_at`; additional packed APIs require an ABI audit. Semantic
Option and Result types remain distinct in checked IR.

Native tools receive literal argument vectors. Private temporary directories own
intermediate files. Successful builds atomically replace the requested output;
failed checks or backend invocations preserve existing outputs. Source/output
aliases are rejected. Normal install/release recipes publish the Rust compiler
as `fern` with `fern-qbe`, `fern-test-supervisor`, `libfern_runtime.a`, and the
package marker; `fern-c` remains an explicit reference executable. The separate
[preview workflow](../docs/RUST_PREVIEW_PACKAGING.md) remains available for
packaging explicit built components. Relocation still requires host native
development libraries for subsequent program compilation.

## Evaluation and maintenance

Run `mise run rust-check` for format, clippy, Rust tests, and native differential
fixtures. CI runs it on Linux and macOS. Run `mise run rust-cranelift-check`
for the optional feature and its independent native corpus, and `mise run check`
for the C reference/runtime and default-command integration gates.
Native fixtures specify exact stdout and exit status independently of C. Known C
backend differences are named in the manifest and reported; they do not relax
Rust's expected output. Seeded generated programs exercise the shared subset.

See [migration progress](../docs/RUST_MIGRATION.md),
[initial evaluation results](../docs/RUST_FRONTEND_EVALUATION.md),
[decision 45](../DECISIONS.md), and the [roadmap](../ROADMAP.md).

## Immutable native JSON

Native Rust programs use opaque `json.Value`/`json.Error` types and the complete
[dynamic JSON API](../docs/JSON_RUST_API.md): validating parse/stringify, exact
numeric conversion, immutable builders and ordered collection access. `Json` is
a compatible module spelling. The old String/Int source signatures no longer
apply to Rust; C source and both legacy native symbols remain unchanged. The REPL evaluates the same JSON API using bounded immutable Rust values,
including retained closures and ordinary Result errors. Its aggregate work and
storage limits are documented in the API reference.

[Typed codecs](../docs/JSON_TYPED_CODECS.md) provide `json.encode(value)` and
`json.decode(text, TargetType)`, including input pipes. Records and newtypes opt in
with `derive(Json)`; concrete generic records and supported containers work in
native programs and the REPL. Unknown fields fail, missing safe Option fields
become None, and `json.error_path` identifies failures with a JSON Pointer.
Regular recursive schemas with finite bases and explicitly derived transparent
newtypes, conditional generic codec-function requirements and explicitly derived
tagged sums and conservatively disjoint unions are supported. General/custom Json
traits and deeper value-based union discrimination remain separate.


Module types and values use separate namespaces. For example, `pub type Id = Int`
and `pub fn Id(value: Int) -> Int: value` can coexist and both are selected by
`import ids.{Id}`. Each declaration keeps its own visibility: publishing one does
not publish a private same-named sibling. A public nominal type publishes its own
constructors; an alias never creates or exports its target's constructors. Calls
and annotations navigate to their respective declarations, while an import
selector can navigate to both. Duplicate declarations within either namespace
remain errors.


The default Rust compiler also compiles `scripts/check_style.fn`. Run
`uv run scripts/test_style_parity.py --compiler compiler-rs/target/debug/fern-rs`
to compare its native diagnostics, severity, file counts and exits with Python.
Source parity and bootstrap workflow checks are included in `mise run rust-check`;
the roadmap records their current acceptance evidence.

Finite ordinary-type unions now support declared member/subset conversions and
typed match narrowing in native execution and the REPL. Full-width payloads retain
nominal and generic identity. Existing containers stay invariant; inferred joins
and lifted operators remain separate work. See [the union contract](../docs/UNIONS.md).


### Syntax inspection

`fern lex source.fn` prints the actual Rust lexer token kinds with UTF-8 byte
ranges. `fern parse source.fn` prints the unresolved source AST, including its
spans. Neither command loads imports, checks types/names or runs code; they also
accept library files without an entrypoint. Lexing still validates delimiters and
indentation, so a successful token dump is not guaranteed for malformed layout.

The escaped text is an inspection format, not stable serialization or byte parity
with the C compiler's internal representation. Dump stdout is uncolored and
unchanged by `--quiet`/`--verbose`; verbose action information goes to stderr.
Only one source path is accepted and `-o` is not supported for these commands.

Input keeps the ordinary 1 MiB source, 65,536-token and parser depth bounds.
Inspection builds a complete dump of at most 16 MiB before publishing stdout;
syntax and resource failures therefore produce no partial dump. Oversized source
is diagnosed before UTF-8 decoding, even if the bounded read cuts a codepoint.
Ordinary read/write failures remain errors. No native backend/runtime is needed.
