# Traits and explicit structural derivation

Morrow traits describe statically dispatched behavior. A trait has one type parameter;
implementation selection happens during checking and specialization. Executables
contain ordinary Rust-backend compiled functions, with no trait object or runtime
method lookup.

```morrow
trait Label(a):
    fn label(value: a) -> String

type Task:
    title: String

impl Label(Task):
    fn label(value: Task) -> String:
        value.title

fn describe(value: a) -> String where Label(a):
    label(value)
```

Bounds remain requirements even when a function does not call a method. All
implementation methods must satisfy their checked contracts, including requirements
inferred from their bodies. Overlapping implementations, missing methods, unknown
bounds, cyclic parent traits and unsatisfied concrete constraints are errors. Trait
resolution has shared work and depth limits. A module may implement a trait when it
owns the trait or the nominal target type. Public traits and methods participate in
ordinary module visibility.

Methods may have default bodies. A child trait declares parents with
`trait Ordered(a) with Eq(a):`. Generic implementations use
`impl Label(Box(a)) where Label(a):`. Method parameter and result annotations state
the contract explicitly. Implementation methods support contiguous pattern clauses;
every clause must satisfy the trait signature and ordinary coverage rules. Named call arguments use the labels on the trait method.

## Built-in value traits

```morrow
type Point derive(Show, Eq, Ord, Clone):
    x: Int
    y: Int
```

- `show(value)` produces readable structural text.
- `eq(left: a, right: b)` compares values structurally; `neq` has a default body.
- `compare(left: a, right: b)` returns `Less`, `Equal` or `Greater`.
- `clone(value)` reconstructs structural values while preserving immutable value semantics.

Derivation is explicit for records, sums and newtypes, including generic and
recursive types. `Ord` requires `Eq`. Records and tuples compare fields
lexicographically; sum variants compare in declaration order before their fields.
String ordering follows UTF-8 lexical order. Lists, options, results and tuples
support these traits when their contents do. Maps support Show, Eq and Clone;
map equality ignores insertion order, while Show preserves iteration order.
Phantom type parameters do not acquire unnecessary bounds.

Float supports Show, Eq and Clone. It has no Ord implementation because ordinary
floating-point comparison does not provide a total order over NaN. Maps likewise
have no inherent Ord. Language operators retain their existing intrinsic semantics;
use `eq` and `compare` for these trait contracts. There are no associated types,
multi-parameter traits or dynamic trait objects in this implementation.

## Verification

`cargo test -p morrow --test traits --test wasm_traits --test language_tour` covers
custom/default dispatch, generic and recursive derivation, module exports,
coherence and missing bounds, Result handling, formatting and combined features.
A deterministic 64-seed test checks derived order and clone/equality against
independent Rust integer/tuple oracles. The native `traits/values.mr` fixture
includes signed 64-bit extrema, recursive values and Unicode. WASM tests exercise
portable string ordering/join and derived values without host imports.
