# Custom JSON codecs

A nominal type can implement Fern's built-in `Json(a)` trait to choose its wire
representation. Both methods return explicit JSON errors:

```fern
newtype UserId = UserId(Int)

impl Json(UserId):
    fn to_json(value: UserId) -> Result(json.Value, json.Error):
        json.from_string("user-{value.0}")

    fn from_json(value: json.Value) -> Result(UserId, json.Error):
        let text = json.as_string(value)?
        let number = json.parse(String.replace(text, "user-", ""))?
        Ok(UserId(json.as_int(number)?))

type Envelope derive(Json):
    ids: List(UserId)
```

`json.encode(Envelope([UserId(42)]))` returns `Ok` containing
`{"ids":["user-42"]}`. Decoding the same text reconstructs the nominal values.
Custom methods execute inside derived records, tagged sums, tuples, maps and lists;
they are ordinary statically specialized Fern functions. An authored implementation
takes precedence over structural derivation for its nominal type.

Generic wrappers can require `Json(a)` and call `to_json`/`from_json` directly:

```fern
newtype Box(a) = Box(a)

impl Json(Box(a)) where Json(a):
    fn to_json(value: Box(a)) -> Result(json.Value, json.Error):
        to_json(value.0)

    fn from_json(value: json.Value) -> Result(Box(a), json.Error):
        Ok(Box(from_json(value)?))
```

Primitive and structurally derived codecs satisfy these bounds. The compiler
bridges direct trait methods to the checked structural encoder/decoder where no
custom implementation is present. These bridges pass through JSON text and its
bounded parser; they preserve full-width integer semantics but currently add
conversion work. Custom implementations require a nominal target; primitive wire
semantics cannot be globally replaced.

## Ambiguity and failures

The compiler treats every custom representation as opaque: its wire shape may be
any JSON value, including null. It therefore rejects `Option(UserId)` and a direct
`UserId | Int` codec when the wire representation cannot be distinguished. Wrap
opaque codecs in a derived record or tagged sum when a protocol needs optional or
union values. Checked outer fields and tags can still establish disjointness.
There is no unchecked user-supplied shape assertion.

An error from a custom method remains a JSON error and composes its path with the containing
wire path. Existing inner paths retain their segments, error code and offset. For example, a
wrong-typed second ID reports `/ids/1`. Ordinary runtime faults still unwind and
run deferred cleanup; they do not become fabricated JSON conversion results.
JSON quota failure from an infallible JSON builder unwinds through a private
boundary and becomes resource error 4 at the enclosing codec operation.

JSON operations invoked by callbacks share the original operation's work,
allocation and node allowances. Nested callbacks have a bounded depth. Catching a
quota error does not restore the allowance. General callback computations remain
subject to their execution engine's ordinary limits; a native codec callback is
ordinary application code, not a separately preempted sandbox.

## Implementation and acceptance

Concrete plans carry checked encoder/decoder function references. Executable
validation checks callback identity, one argument, exact result type, absence of
captures and absence of actor mailbox effects before execution. Native descriptor
bridges preserve full-width payloads and distinguish managed roots from scalar
bits; interpreter callbacks retain safe Rust values. Plan traversal preserves the
existing finite-shape and conservative union proofs.

`cargo test -p fern --test custom_json` exercises nested records/lists, generic
bounds, error paths, callback faults and cleanup, caught quota failures, bounded
recursive callbacks, and 64 deterministic full-width round trips against independent
integer oracles. `custom_json_native/values.fn` supplies a separate native stdout
oracle. Custom JSON execution is available on native and REPL targets; the current
WASM subset does not implement the JSON runtime APIs.
