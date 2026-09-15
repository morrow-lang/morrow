# Typed JSON codecs in the Rust compiler

`json.encode(value)` returns `Result(String, json.Error)`.
`json.decode(text, TargetType)` returns `Result(TargetType, json.Error)`.
Both operations require handling their Result. `Json.encode` and `Json.decode`
are aliases. The existing dynamic JSON API is unchanged.

Records opt in explicitly:

```morrow
type User derive(Json):
    age: Int
    name: Option(String)

fn main() -> Result(Unit, json.Error):
    let user = json.decode("\{\"age\":42\}", User)?
    println(user.age)
    println(Option.is_none(user.name))
    println(json.encode(user)?)
    Ok(())
```

The decoder's second argument is a compile-time type reference. It uses the type
namespace, including imported types and transparent aliases; it never evaluates a
same-spelled function or value. Arbitrary expressions are not type targets.
Input pipes work, for example `text |> json.decode(User)`.

Supported concrete wire types are Int, Float, Bool, String, Unit, `json.Value`,
tuples, Lists, `Map(String, value)`, nullable-safe Options, and regular recursive
records, tagged sums and transparent newtypes marked `derive(Json)`, and unions
with provably disjoint wire shapes. Concrete
instantiations of generic records, sums and newtypes are supported.
Records retain declaration order; maps retain their existing entry order.
Unknown record fields fail strictly. A missing Option field becomes None; other
missing fields fail. None and Unit encode as JSON null. Option payloads must not
themselves accept null, so `Option(Unit)`, `Option(json.Value)`, and nested Options
are rejected. Int conversion preserves the full signed 64-bit range; Float and
text conversion reuse the dynamic API's existing exact adapters.

This checkpoint does not implement general Json traits or user codec implementations.
Unsupported derivations are diagnosed even when unused. Result-bearing values,
including nested fields or containers, cannot be serialized: converting a Result
to opaque JSON does not acknowledge its error obligation.

## Errors and paths

Existing error codes, messages, numeric conversion rules and parse offsets remain
unchanged. Code 12 means `unknown JSON object field`. `json.error_path(error)`
returns a JSON Pointer: root is empty, array indices are decimal, `~` becomes
`~0`, and `/` becomes `~1`. A key `a/b~c` containing a failing item at index 1
therefore reports `/a~1b~0c/1`.

Conversion errors have offset -1; parse errors preserve the original byte offset
and have an empty path. Unknown fields report their key path. JSON keys containing
NUL report code 10 at the containing object. Missing required fields report code 6
at the field path. Path storage is reserved before descent; if path growth exceeds
a codec limit, code 4 reports the last successfully entered parent path. Reporting
a path never replaces an already selected error's code or offset.

## Limits and verification

Each codec operation shares one allowance across parsing, conversion, primitive
adapters, paths and output: 1 MiB input, 16 MiB output, 32 MiB logical allocation,
100,000 expanded nodes, depth 128 and 64 MiB work units. These are deterministic
logical accounting limits, not a guarantee against physical allocator failure.
Interactive execution additionally retains its existing per-entry, cleanup and
stored-value allowances. It may report an interactive resource fault before a
native codec limit.

Concrete compiler plans contain at most 4,096 entries. Plan validation shares
400,000 work units across an executable program; repeated immutable plan identities
are reused. Native descriptor output has a separate aggregate 16 MiB limit. These
bounds also apply to independently supplied executable IR, including inactive
function bodies. Source type targets never enter executable IR.

`cargo xtask check` verifies native source behavior, atomic rejection, runtime ABI,
shared sibling allowances, exact paths and preservation of original errors.
The same source corpus is exercised in the REPL. Rust runtime tests also force
precise collection during nested decoding. Existing dynamic JSON runtime and
numeric-oracle gates remain required; historical sanitizer evidence predates
the current Rust implementation.

## Regular recursive records

A derived record may refer back to itself or to another derived record through
List, Map or a nullable-safe Option. Concrete generic instances retain separate
schema identities. The compiler closes a finite indexed graph rather than
expanding a recursive record into an infinite tree. Finite generic permutations
can close too; type-changing recursion that exceeds the existing type/plan/work
bounds is rejected.

Every declared codec must admit a finite value. `Node { children: List(Node) }`
has the empty-list base case. A strict cycle such as `Loop { next: Loop }` has no
finite value and is rejected as a codec, even when unused. Other fields on a
recursive record are still validated, so a cycle never hides a Result or an
unsupported function payload.

Schema reuse does not replenish runtime allowances. Each executed visit spends
the existing depth/work/node budgets. A repeated record/list chain at depths
127 and 128 preserves the same success/error boundary in native and interactive
execution; a hostile native cyclic value terminates with code 4. The path and
original-error preservation rules above continue to apply.

## Transparent newtypes

Newtypes opt in with an explicit header, for example
`newtype UserId derive(Json) = UserId(Int)` or
`newtype Box(a) derive(Json) = Packed(a)`. A type without this opt-in has no codec.
Encoding and decoding use the payload's wire form while preserving distinct
source type identity. Native constructors, accessors and codec adaptation add no
wrapper allocation, including for nested Float payloads. Existing numeric/text
conversion rules are unchanged; Float JSON uses the established 17-digit format.
The REPL also erases the wrapper representation, retaining its checked source type.

Nullability follows the payload. A newtype wrapping Option(Int) accepts explicit
JSON null. It is still a **required** record field: missing-field defaults apply
only to actual Option fields. Conversely, Option of a newtype that accepts null
is rejected as ambiguous. Newtype identity does not grant implicit Json support
to Map keys: they remain exactly String.

Regular recursive newtypes such as a wrapper over List of itself share the same
finite-graph and runtime-budget rules as recursive records. Strict unboxed cycles
remain invalid. Every transparent wrapper layer spends work and depth, even
though it adds no native allocation. Result-bearing payloads remain unsupported.

## Generic codec functions

Source wrappers retain conditional codec requirements through inference, explicit
generic signatures, recursive calls and function values:

```morrow
fn write(value): json.encode(value)
fn read(text: String) -> Result(a, json.Error): json.decode(text, a)
```

Each concrete call must satisfy the retained requirements. `Json` requires a
supported wire representation; `JsonNonNull` additionally excludes null for Option
payloads; `JsonStringKey` requires exactly String for Map keys. These are inferred
compiler capabilities, not new source `where` syntax or user-defined traits.
Source wrappers can be passed as callbacks; decode's target remains static.

Requirements follow actual stored fields. An unused phantom type parameter creates
no codec requirement, even when it names a function or Result. A real stored
function or Result still rejects, including in an unused derived declaration.

Generic checking retains real input effects in a private codec template. Concrete
specialization validates the exact target again and creates the ordinary complete
codec plan. Templates cannot be executed, lowered, or retained in REPL state;
public IR validation rejects them even in inactive code. There is no default
concrete witness. Predicate checking and generic-template validation each share a
400,000-unit allowance across their respective pass; concrete plan and runtime
limits above remain unchanged.

## Tagged sums

Derived sums opt in with the existing `type Event derive(Json):` syntax. A constructor `Count(42)` encodes exactly as `{"tag":"Count","fields":[42]}`; a nullary `Ready` encodes `{"tag":"Ready","fields":[]}`. The tag is the original unqualified source constructor token. Declaration order, module aliases and nominal type spelling do not appear in the wire identity. Rename a constructor or reorder its payloads only when a wire-format change is intended. Output keys are always tag then fields; payloads retain source declaration order.

Both envelope keys are mandatory. Unknown input keys fail first in input order (code12); then missing tag/fields fail in that order (code6); then tag type/name, payload-array type/arity and child conversions are checked. Unknown constructor names produce code13, `unknown JSON variant`, at `/tag`. Payload failures retain primitive code/offset and append `/fields/<index>` to the current path. No candidate payload is converted before complete envelope shape validation. Tagged source ordinals never become text.

Generic and mutually recursive sums use the existing conditional Json requirement machinery. Finite values are proved as an OR of constructor AND-products. A nullary base permits a recursive chain; a type with only a strict self-recursive constructor is rejected as a codec restriction. Every stored component in every variant is checked, even when another variant supplies a finite base. Stored Results/functions remain unsupported. Phantom type arguments do not create stored fields or obligations.

The native codec descriptor remains four 64-bit words. For kind12 only, its third word is a typed pointer to three-word variant descriptors (source-name pointer, payload count, codec-pointer array). Other kinds retain their existing child-pointer meaning. This is the narrow Decision103 external ABI exception, not a general language representation change. The new TypeLayout.variant_names vector independently validates source spelling/native tag order; plan products are never fabricated tuples.

Envelopes count their real object, tag text and array nodes against existing JSON depth/node/output/storage limits. Consequently fewer recursive sum links fit within depth 128 than recursive records with fewer wire layers. Plan/metadata/proof work remains one 400,000-unit allowance, source plan count 4096, constructor count 255, descriptor output 16 MiB. Child conversion, tag scans, pointer-path growth and final stringify share the original runtime allowance. Failed path growth retains the parent; an earlier error's code/offset/path cannot be overwritten.

## Disjoint union codecs

Inline static targets accept `json.decode(text, Int | String)` and nested forms
such as `List(Int | String)`, `Map(String,Int | Bool)`, `(Int | String,Bool)` and
`Box(Int | String)`. `type Choice(a) = a | String` and
`json.decode(text, Choice(Int))` remain equivalent. Input pipes support the same
target grammar. These forms are type syntax only in a canonically resolved codec
slot; an ordinary function or a shadowed receiver cannot consume them as values.
Existing lambda and runtime-expression targets remain invalid. Recognition is
linear in the token stream: argument boundaries and token-prefix counts are built
once, then queried in constant time. Construction and queries share a precharged
400,000-unit allowance. The 1 MiB source/65,536-token limits and existing type
depth/union-alternative limits of 128 remain.

This does not change source Map key policy: a structural union cannot itself be a
source Map key. Generic `Map(k,v) | String` retains the exact JsonStringKey(k)
requirement until independently inferred concrete instantiation.

Encoding forwards only the selected member's wire value. Decoding selects exactly
one member before invoking its decoder. Strict records use actual required and
allowed keys, with optional status only for actual Option fields. A nullable
newtype field remains required. Exact tuple lengths and distinct source
constructor tags can discriminate. Newtypes inherit their payload's wire shape
without merging nominal source identity.

Nested required fields and tuple positions can also discriminate. For example,
records with `payload.value: Int` versus `payload.value: String`, or tuples
`(Int, String)` versus `(String, Int)`, are disjoint. At least one record branch
must require the discriminating field: two optional fields can both be absent.
Int and Float overlap, arbitrary List shapes overlap at the empty list, and
dynamic JSON overlaps every kind. Recursive schemas need a finite discriminator;
a repeated recursive pair alone proves nothing. Shared sum tags can use payload
arity or nested payload shapes: `Event(Int) | Event(String)` is valid when the
only constructor is `Payload(a)`. Adding a shared zero-field `Empty` constructor
makes those alternatives overlap and is rejected. The proof remains conservative
for unconstrained list elements and map values.

Unique JSON-kind selection preserves original numeric/text errors. All-sum object unions select a known source tag before strict envelope errors. Mixed record/sum unions use strict key sets first. No unique member is code14 `no unique JSON union member`, at the current parent path with offset -1. Existing standalone sum code13 and codes1–12 are unchanged. No candidate decoder or JSON Pointer allocation is used for selection.

Only when kind and shallow structure leave multiple candidates does selection
inspect borrowed nested shapes. Once selected, the member decodes exactly once.
Numeric conversion still owns precision and range errors; structural selection
does not try conversions or choose a fallback based on declaration order.

Compiler profile propagation, pair comparisons and type/name work consume the existing aggregate400k allowances, with precharged queues/storage. Concrete plans validate inactive entries too. Native selectors share the original64MiB work allowance with decoding and perform no candidate allocations. Existing input/output/allocation/node/depth limits remain; transparent dispatch adds charged codec steps.
