# Result handling in the Rust compiler

Every produced Result carries a duty to acknowledge its success/error choice.
Results inside either payload carry separate duties. A successful function exit
must handle every locally produced duty, return it to the caller, or run a
registered defer that handles it. This includes early `return`, `?` and `with`
error exits.

References alone do not handle errors. Reading a list's length, inspecting Option
metadata, or passing a Result to a borrowing helper leaves its duty pending:

```morrow
fn count(values: List(Result(Int, String))) -> Int: List.len(values)

fn main():
    let values: List(Result(Int, String)) = [Ok(1), Err("disk")]
    println(count(values))
    for value in values:
        match value:
            Ok(number) -> println(number)
            Err(message) -> println(message)
```

The count helper is valid because it borrows its input. Removing the final loop
would leave the caller's errors unhandled. A deliberately unused Result-bearing
parameter still fails the independent unused-input rule.

## Aliases and nested values

An immutable alias retains the original duty; handling either alias handles that
same Result. Repeated handling is allowed. Distinct constructions remain distinct.
If a branch selects one of two existing Results, handling the selection does not
handle the unselected Result. A fresh Result produced on each branch can be handled
after the branches join because only that branch's duty exists.

Exhaustive matching handles the Result tag being tested and preserves duties in
bound payloads. `Result.is_ok` and `Result.is_err` explicitly acknowledge the outer
tag only. A nested Result still needs its own handling. Wildcard-only matching,
ordinary metadata and opaque serialization do not acknowledge errors.

`?` handles its input's outer tag and transfers the Err payload to the caller.
Other existing duties must already be handled or covered by cleanup on that early
exit. Returning a complete collection transfers its contained duties; returning
one selected item does not transfer the others.

## Collections and callbacks

A complete `for`, `List.map` or equivalent traversal proves handling only when its
body handles every visited element on every relevant path. `find`, `any`, `all`,
`head`, selected indices, filtered subsets and early loop exits cannot prove full
coverage. A later full traversal of the original collection remains valid.
Map replacement/deletion cannot erase duties in omitted values; retain and handle
an alias or handle those values before replacement. Fold callbacks must handle or
retain each old accumulator, including duties produced after the initial value.

Mapping an operation first creates a collection of Results. Applying `?` to each
item in a later loop may abandon the already-produced suffix on the first Err.
Handle each item explicitly, or arrange complete transfer/cleanup of the remaining
values. The generic JSON callback fixtures use exhaustive per-item handling; a
paired regression keeps the early-propagation rejection covered.

Source function summaries preserve actual callable targets and their handling
effects. A function type alone grants no handling guarantee. Generic functions
retain conditional effects and every demanded specialization is checked again.
Ordinary closures cannot capture existing Result-bearing values; guaranteed defer
handlers retain their separate supported capture rule. Conditional registration
protects only paths where it occurs.

## Boundaries and current limits

The checker validates unused bodies, generic templates, concrete specializations
and dependencies before publishing successful semantic facts or REPL state. A
rejected REPL entry performs no new effects and preserves prior accepted state.
Source-only label suggestions do not constitute successful type checking.
JSON conversion borrows its real input and creates a fresh Result; actual stored
Results remain ineligible for serialization. Phantom type arguments create no
stored obligations.

The proof shares 400,000 work units, with at most 32,768 Boolean predicate nodes
and bounded depth, sequence positions and callable choices of 128. Exhaustion or
an unsupported proof produces a diagnostic, never handling credit. It checks
ordinary source exits, not forced process termination, allocator faults or general
termination.

Exact recursive aliases and direct handlers over a strict suffix of their own
list input have checked inductive contracts. Recursive nominal trees containing
stored Results support complete handlers of their own strict descendants, including
List/Map child traversal, Option/newtype edges, exact recursive callbacks and
per-child deferred handling. Equal types, reconstructed roots and partial searches
confer no structural evidence. Every sibling and locally produced Result must be
handled independently. Whole-tree aliases preserve the actual duties.

Map emptiness keeps its actual conditional identity across full traversal and
Map.values. Deleting a dynamically selected key may empty a map; it cannot reuse
the old nonempty fact or erase removed Result duties.

Mutually recursive complete Unit handlers may form a checked group. Every member
must accept exactly one Result-bearing recursive nominal input, and every call
within the group must receive a complete strict descendant of that member's own
input. Different node types and parameter positions are supported, including
complete List/Map traversal and exact callbacks. Every member proves all branches
before outside callers receive handling credit. Guarded aliases retain actual
branch conditions; equal layouts, unrelated captures and reconstructed nodes do
not establish descent. An iterative dependency graph and ancestor certificates
use the same aggregate proof budget.

Recursive builders use bounded inductive contracts for fresh closed outputs and
complete retention of Result-bearing inputs in List or nominal accumulators.
Aggregate subtree obligations and duty-free accumulator elements never prove a
returned list is nonempty. This can conservatively reject flattening opaque
recursive children, even when a human can prove actual leaf duties are present.
Exact alias contracts take priority; widening a mutually recursive group rebuilds
dependent summaries within the original shared work budget. A provisional
contract grants no handling credit, and each successful exit must retain every
promised input duty. Generic and mutually recursive builders share these rules.

The checker deliberately rejects equations it cannot prove: unknown-key Map
overwrite/freshness, consuming or replacing accumulators, and arbitrary recursive
higher-order transformations. These are conservative proof boundaries, not
permission to ignore a Result. Origin-free recursive JSON payloads are supported;
their enclosing codec Result still requires handling.

Typed child traversals also preserve transparent newtype wrappers, including nested
wrappers. Projecting the stored child retains its identity; constructing a wrapper
around the original root never supplies descent evidence. Native regressions cover
alternating node types, deferred child handling and wrapped Map children.
