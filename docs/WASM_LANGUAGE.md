# Portable Fern language execution

The browser backend compiles checked Fern IR directly to core WebAssembly. It
needs no WASI, native runtime, linker, JavaScript implementation of the language,
or imported host functions. Browser I/O remains the responsibility of the Rust
host. Native actors, databases, filesystem/network APIs, foreign calls, runtime
JSON codecs and some native string/math helpers are separate capabilities and
remain rejected when used in a portable module.

## Language features

In addition to scalar functions, UTF-8 strings, records, tuples, lists and tagged
sums, the portable backend executes:

- Captured and escaping closures, function values, higher-order functions and
  generic specializations, including full-width integer and float captures.
- Immutable insertion-ordered maps and sets. Updating a map preserves its source;
  replacing a duplicate key preserves the first insertion position.
- List map/filter/fold/find/any/all/contains/enumerate; Option.map and
  Result.map/and_then/unwrap_or_else with typed callbacks and short-circuiting.
- First-class lazy ranges and iteration over lists and maps, with lexical
  break/continue and full-width inclusive endpoint handling.
- Result propagation with `?`, typed `with` handlers, early return, and dynamic
  function-owned LIFO `defer` callbacks, including callbacks registered in loops.
- Structural union injection, widening and subset patterns. Each envelope traces
  only its selected managed payload; scalar bits never become guessed pointers.

A range with a start greater than its end performs zero iterations. Inclusive
`9223372036854775807..=9223372036854775807` executes once and terminates before an
increment can wrap. Hosts must meter work: a finite i64 range can still be vast.

## Ownership, faults and host ABI

Closures use precise aggregate environments containing a checked function tag
and captured fields. Invocation dispatches only to matching checked signatures.
Captured implementation functions have no public raw-environment export. Multiple
specializations sharing one source name export as
`name::specialization::<checked-function-id>`; unique names retain their existing
exports. Managed signatures retain the `fern::` prefix and generational i64
handle ABI. Handles may represent closures, maps, sets, ranges and unions as well
as the previously supported values.

Transient roots are reset between loop iterations and higher-order callback
steps. A partial collection, current fold accumulator, escaped closure and
registered cleanup chain remain precisely traced across allocation. Collection
storage retains full-width payload bits and an explicit child bitmap.

Generated language faults, including checked arithmetic, collection bounds,
allocation limits and callback faults, propagate through function cleanup before
the host entry traps. A failing cleanup does not prevent later callbacks from
running. A subsequent host entry starts with fresh transient roots and fault
state. External termination, such as host fuel exhaustion or WebAssembly call
stack exhaustion, is an immediate engine trap and cannot promise cleanup. It is
not a catchable Fern Result.

## Bounds and acceptance

The existing preview ceilings remain: 4,096 bytes per string, 511 fields/items
per aggregate/list/map, 8,192 slots for aggregate modules (256 for string-only
modules), 16,384 transient root slots and 256 host handles. Memory has a fixed
maximum and does not grow. Reaching a bound fails instead of silently truncating
values. Map lookup is linear and repeated immutable insertion can be quadratic.
Compiler bounds also apply to function counts, generated locals, expression/type
complexity and final module bytes.

`cargo test -p fern --test wasm` exercises independent Wasmi expected values and
existing full-width/UTF-8/handle/GC oracles. The added deterministic campaign uses
seed `0x4645524e`, eight runs of 128 map edits, an independent Rust ordered-map
model, all key/value positions and wrapping sums after each edit. Another test
keeps an escaped closure handle alive through 9,000 allocation-heavy loop turns.
A deterministic Wasmi instruction meter verifies that remaining deferred loops
execute after both body faults and cleanup faults. Capacity faults and stale
handles are followed by successful calls in the same instance.

These are reproducible correctness and failure-boundary checks, not a claim of
production battle-hardening or equivalence to years of real workloads. Real
browser and complete native acceptance remain separate integrated gates.
