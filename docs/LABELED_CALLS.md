# Source call labels

The Rust frontend accepts labels on direct source function calls. Labels name
parameter positions; expressions still execute once, in the order written.

```fern
fn subtract(left: Int, right: Int) -> Int:
    left - right

fn main() -> Int:
    subtract(right: 2, left: 9)
```

This returns 7. Positional arguments may form a prefix, followed by labels in any
order. Unknown, duplicate, missing or already supplied positions are errors.
Exact Bool parameters and parameters sharing an identical declared scheme type
require labels. The interface is fixed after declaration inference and alias/union
normalization, before call-site specialization. Distinct generic variables and
distinct newtypes remain distinct; a union merely containing Bool is not exact
Bool. Repeated instances of the same generic parameter do require labels.

For example, both parameters of `subtract` above require labels. A unique String
parameter can still form a positional prefix before labeled Int/Bool parameters.
The rule applies to direct source functions; erased and builtin interfaces are
described below.

A simple parameter binding supplies its external name. Pattern parameters can
declare one explicitly, for example `fn choose(enabled true: Bool) -> Int: 1`.
All clauses expose a stable agreed interface; literal/wildcard clauses may inherit
it from another clause. Conflicting names need consistent explicit external names.
Required positions without an agreed name must declare an explicit external label.
External labels and local pattern bindings occupy distinct source roles.

A pipe evaluates its input before the written call arguments. A labeled placeholder
such as `9 |> subtract(right: 2, left: _)` chooses its parameter explicitly.
When the selected parameter requires a label, the pipe must use a labeled hole.
Reordering does not move side effects, returns or Result propagation across other
arguments. Imported functions and reexports retain their original interfaces.

Labels erase when a function becomes a structural function value. Those values,
lambdas, runtime/compiler builtins and constructors use positional calls and reject
labels. These source interfaces are checked by Fern's Rust frontend.

Formatting roundtrips labels, external pattern names and labeled pipe holes.
LSP definition requests on labels select the original parameter interface, independently
of same-spelled caller locals and internal pattern bindings. Hover shows the finalized
declared parameter type, including generic identities. Both require the complete
current source graph to check successfully. Imports, reexports and unsaved dependency
buffers retain exact source and UTF-16 identities. Incomplete-member recovery also
preserves mandatory-label validation in unaffected functions.

The Rust LSP uses the compiler parser for label navigation and completion.
`cargo xtask compatibility` executes the native label programs and verifies that
invalid programs preserve existing outputs, permissions and modification times.
Rust tests cover modules, contextual callback types, source spans, formatting,
presentation and hostile AST budgets.
Public AST labels share the parser's identifier/keyword rules and reject reversed
spans. Finalized scheme classification has a separate 400,000-unit work ceiling.

For incomplete calls, LSP completion offers source parameter names from the current
module graph, including unsaved dependencies. It excludes supplied positions and
respects local shadowing, external pattern names and original pipe placeholders.
The label edit replaces the exact UTF-16 name range while retaining an existing
colon/value. These suggestions describe source names, not checked parameter types
or mandatory labels; ordinary value completion remains available when no source
label matches. Inconsistent clause interfaces and builtin/constructor collisions
provide no source-label suggestions.

Recovery supports closed calls and unfinished calls at the end of the file when
only parentheses remain unmatched and the suffix is whitespace. Other malformed
syntax does not gain invented executable expressions. Completion publishes at most
256 items within a conservative 1 MiB edit budget, marking truncation explicitly.
Twenty protocol regressions cover these boundaries, current overlays, imports,
Unicode/CRLF edits and lexical fallback.
