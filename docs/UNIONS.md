# Finite unions in the Rust frontend

A union accepts any of its ordinary member types. Narrow it before using a
member-specific operation:

```morrow
type Value = Int | String
fn describe(value: Value) -> String:
    match value:
        n: Int -> "number: {n}"
        s: String -> "text: {s}"
fn main():
    println(describe(4294967296))
    println(describe("morrow"))
```

Unions flatten, deduplicate and canonicalize after alias expansion and generic
substitution. `Int | Int` becomes `Int`; distinct newtypes remain distinct.
`(Int) -> String | Bool` is a function returning a union. Write
`((Int) -> String) | Bool` for a union containing a function.

Declared parameter, return and local annotations permit member injection and
subset widening. Fresh collection literals and lambdas can use an unambiguous
compatible member supplied by that context. Existing containers and function
types remain invariant: a `List(Int)` cannot be relabeled `List(Int | String)`.
Inferred branches and repeated generic parameters require exact equal types;
the compiler does not infer a union from unrelated values or guess an ambiguous
generic membership. Equivalent generic sets match independently of source order.

Typed match binders and wildcards select a member or nonempty subset. An
unguarded selection covers its selected members; a guard does not establish
coverage. Useful overlapping subsets are allowed, while unreachable arms and
missing members are errors. Generic source coverage is checked before concrete
specialization, which removes only arms made redundant by member collapse.

Arithmetic, equality, printing, interpolation, List.contains and Map-key use
require narrowing first. A Result-bearing member still requires handling;
wildcard discards and delayed closure captures cannot bypass that obligation.
The `?` operator applies to an actual Result, including a Result whose error
payload is a union. Ordinary and test entry points preserve those payloads.

Native carriers use a managed 16-byte envelope: a 64-bit member tag followed by
64 payload bits. Floats, full-width integers, closures, nominal values and native
pointers retain their existing representation inside that payload. Subset
conversions remap tags into fresh envelopes after evaluating the source once.
Canonical tag order is a compiler implementation detail, not a stable external
ABI. Singleton collapse removes the carrier. Interactive values retain semantic
member identity and participate in the REPL's bounded shared storage accounting.

Type nesting is capped at 128, individual structures at 4096 nodes, and union
sets at 128 alternatives. Union/newtype normalization and comparison share a
400,000-unit work budget; coverage uses the existing 20,000-work profile.
Backend validation also bounds inactive signatures, conversions and pattern
metadata before cloning or queuing children. Private inference/editor nodes
remain forbidden in executable IR.

Constructor refinements such as `Ok(data) | Err(msg)`, variance, implicit union
joins and lifted capabilities remain separate work. The formatter, module loader,
documentation and semantic language server support this checkpoint; full editor
parser parity is tracked separately. This feature uses the default Rust compiler,
`morrow`. The explicit C reference compiler was removed during the Rust-only migration.
