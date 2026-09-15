# Immutable sets

`Set(a)` is a distinct immutable collection of unique values. Sets preserve first
insertion order for iteration through `Set.to_list`; reinserting an existing
value keeps its position, while deleting and reinserting moves it to the end.
Equality through `Set.equal` compares membership and ignores insertion order.

```morrow
let languages = Set.from_list(["Morrow", "Gleam", "Morrow"])
let expanded = Set.insert(languages, "Elixir")
println(Set.len(languages))  # 2; the original remains unchanged
println(Set.contains(expanded, "Elixir"))  # true
```

| API | Behavior |
| --- | --- |
| `Set.new()` | Empty set; provide element context, such as `let s: Set(Int) = Set.new()` |
| `Set.from_list(values)` | Deduplicate a list in first-occurrence order |
| `Set.to_list(set)` | Elements in insertion order |
| `Set.insert(set, value)` | Return a set containing the value |
| `Set.delete(set, value)` | Return a set without the value; absent values are harmless |
| `Set.contains(set, value)` | Test membership |
| `Set.len(set)` | Number of unique elements |
| `Set.is_empty(set)` | Test whether there are no elements |
| `Set.union(left, right)` | Left elements, followed by previously absent right elements |
| `Set.intersection(left, right)` | Elements present in both, in left order |
| `Set.difference(left, right)` | Left elements absent from right, in left order |
| `Set.is_subset(left, right)` | Every left element occurs in right |
| `Set.equal(left, right)` | Identical membership, regardless of order |

`value in set` also tests membership, evaluating the value before the set.

These APIs accept positional arguments, support pipes and can be passed as
function values. Type aliases and generic functions preserve the distinct set
identity. `Map` and `List` values cannot be used as sets implicitly, and set
storage has no public constructor or `.0` field.

Elements currently share the implemented map-key equality contract: `Int`,
`Bool`, `String`, and nominal newtypes whose underlying representation is one
of those types. Integer equality preserves all 64 bits; strings compare exact
UTF-8 contents, without Unicode normalization. Floats, records, lists, functions
and `Result` elements are rejected. Supporting a user-defined equality trait does
not by itself establish a valid map/set key implementation.

The current backing storage shares immutable map operations and collector roots.
Lookup is linear and repeated insertion or algebra can take quadratic time. This
is a correctness-first implementation, not a claim of hash-set performance.
Native and interactive tests cover independent membership models, historical
aliases, Unicode and integer boundaries. A native regression explicitly forces
precise collection before map operations, including nested allocation helpers;
ordinary runtime collection still retains its documented conservative boundaries.

Browser acceptance also executes Set.from_list, union, intersection, difference,
subset and equality in Wasmi, including insertion order, Unicode and full-width
integer keys against independent expected results.
