> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

> Historical document, retained for architectural context. Its implementation paths, commands and measurements describe the retired setup. See [the current build guide](../../BUILD.md) and [roadmap](../../ROADMAP.md).

## Decision 78: Current-source completion for one incomplete member

The Rust editor can recover one member operation at the completion caret when its
receiver has independent concrete type evidence. This is a completion-only partial
proof, not successful ordinary compilation. Hover, definition, executable IR, and
reusable type schemes retain their existing complete-source requirements.

The actual lexer identifies a Dot followed by an empty selector or the selected
identifier/tuple-slot token. The parser substitutes a private token in its token
stream, preserving all original source bytes and spans. It builds the real receiver
expression and an opaque site identifying exactly one empty-selector Field. No
source text, existing field, Unit value, Never value, or inference Probe substitutes
for the missing operation. Comments, literal text, annotations, unrelated malformed
syntax, and a second incomplete operation do not become recovery sites.

Recovery is initially restricted to a nongeneric enclosing function group whose
parameter and return types are independently explicit and concrete; main retains
its existing Unit default. Every clause has to satisfy that rule. An incomplete
recursive peer also prevents recovery, because SCC inference could otherwise inspect
the selected body. Omitted or quantified outer signatures get lexical fallback until
a separately specified transitive dependency-taint design exists. This restriction
does not prevent ordinary local inference, concrete callbacks, lexical captures,
pattern binders, alias expansion, or calls to independently checked generic helpers.

The declaration graph, aliases, reusable schemes, and unaffected nongeneric bodies
are checked using the ordinary rules. At the selected member site the checker must
resolve the receiver fully concretely before allocating the missing result variable.
Only then does it create an opaque non-executable EditorHole IR node. The selected
body continues through normal local constraints, control flow, discarded-Result,
pattern coverage, and finalization checks. Unresolved equivalence classes reachable
from the hole result may be sealed as private rigid editor markers for this one
local proof. They never justify receiver evidence, become quantified schemes, or
reach specialization. Unrelated unresolved variables and unrelated source errors
still reject the proof. Complete known selectors cannot mask an unrelated error.

Only the independently concrete receiver and its real record fields, tuple slots,
or existing List.enumerate method are returned. No hole result or editor marker is
published. Opaque TUI and JSON values expose no invented fields. The EditorHole token
has no public constructor; ordinary finalization, IR publication, QBE emission,
REPL evaluation and retained REPL program storage explicitly reject such nodes,
including syntactically unreachable ones at publication/storage boundaries.

Each request uses current source and current unsaved module overlays. No repaired
source, last-good graph, candidate-by-candidate compilation, or guessed receiver
name is used. Original byte positions become UTF-16 text edits, including partial
whole-token replacement, Unicode, CRLF, and interpolation. Closing an overlay
restores disk evidence. Failed proofs retain existing lexical completion fallback.

Existing file/graph/token/depth/inference/type and metadata budgets remain active.
Completion remains deterministic with at most 256 items and the existing encoded
text budget and isIncomplete indicator. Records retain the compiler's existing
255-field limit; recovery must not accept an over-limit record merely to supply
suggestions. Valid maximum-size and over-limit rejection cases are tested.

Editor libraries are checked without synthetic entry declarations. Free main
references remain errors; real local bindings and imported functions retain
ordinary name resolution. The loader pins the parsed entry snapshot before
reusing its AST, so source coordinates cannot drift after a second disk read.
