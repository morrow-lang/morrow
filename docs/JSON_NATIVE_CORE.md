# Immutable JSON native core (J1)

This document defines the native parser/accessor foundation. J2 adds immutable
builders and collection adapters, exposed by the [Rust native API](JSON_RUST_API.md).
The legacy `fern_json_parse` and `fern_json_stringify` string-copy entry points
were removed during the Rust workspace migration. The Rust REPL implements the same dynamic
profile with additional interactive aggregate/retention limits; typed codecs remain
separate. See [the Rust API](JSON_RUST_API.md).

## ABI and ownership

`crates/fern-runtime/src/json.rs` exports opaque value/error pointers through
the native ABI. Handles are owned by Fern's Rust collector; finalized handles
release their shared Rust JSON storage. Callers must not free them individually.
Values are immutable.
The C API requires valid, non-NULL opaque pointers returned by this API. It does
not validate forged C pointers or provide mutable access to internal fields.

All functions below use the `fern_json_value_` prefix:

| Suffix | Parameters | Result |
| --- | --- | --- |
| `parse` | `const char*` | `Result(Value*, Error*)` |
| `stringify` | `const Value*` | `Result(String, Error*)` |
| `is_null` | `const Value*` | `int64_t`, zero or one |
| `get` | `const Value*, const char*` | `Result(Value*, Error*)` |
| `at` | `const Value*, int64_t` | `Result(Value*, Error*)` |
| `length` | `const Value*` | `Result(Int, Error*)`, array/object only |
| `as_bool` | `const Value*` | `Result(Bool, Error*)` |
| `as_int` | `const Value*` | `Result(Int, Error*)` |
| `as_float` | `const Value*` | `Result(Float bits, Error*)` |
| `as_string` | `const Value*` | `Result(String, Error*)` |
| `number_text` | `const Value*` | `Result(String, Error*)` |
| `error_code` | `const Error*` | `int64_t` |
| `error_offset` | `const Error*` | `int64_t` |
| `error_message` | `const Error*` | static `const char*` |
| `error_path` | `const Error*` | immutable JSON Pointer `const char*` |

Every Result is the existing full-width heap Result returned as `int64_t`.
Success/failure tags are read with `fern_result_is_ok`; the full-width payload is
read with `fern_result_unwrap`. Pointer payloads convert through `intptr_t`.
Float success payloads are binary64 bit patterns, not integer numeric
conversions. These are ordinary Result failures, and normal source Result
handling applies.

## Format profile

Parsing accepts scalar roots, JSON's four whitespace bytes, and at most one
leading UTF-8 BOM. Offsets include the BOM. Comments, trailing content, trailing
commas, malformed numbers and other whitespace are rejected. Strings require
Unicode scalar UTF-8; escaped surrogate pairs decode to one scalar, and unpaired
surrogates are rejected. A malformed scalar reports its first byte (a malformed
surrogate pair reports the first backslash). Incomplete or nonhex `\u` syntax
reports the first syntax failure instead.

Objects preserve input member order and reject duplicate **decoded** names,
including differently escaped names. A stable sorted index supports lookup
without changing serialization order. Duplicate errors identify the later
member's opening quote within the first duplicate group encountered in sorted
key order. `get` distinguishes missing names from wrong value kinds. Empty names
and prefix-related names remain distinct. `at` checks negative and large signed
indices before reading array storage. `length` never treats a String as an array.

Decoded JSON strings and names retain lengths and can contain escaped NUL.
Stringification re-escapes NUL as `\u0000`; `as_string` returns code10 if exposing
the string through Fern's current NUL-terminated String ABI would truncate it.
C input parameters are NUL-terminated; embedded input bytes after the terminator
are outside that ABI. `get` therefore cannot address a NUL-containing key; such
keys remain preserved by parse/stringify, through the lossless `json.members` adapter as JSON String values.

Numbers preserve the entire validated ASCII spelling. Parsing never rounds
through Float. `number_text` and stringification preserve exponent spelling,
trailing fractional zeros, negative zero, and arbitrary precision within limits.
`as_int` analyzes decimal digits/exponents exactly, accepts mathematically
integral values, and rejects fractions or signed64 overflow. Exponents saturate
for analysis, so their magnitude never controls loop counts. `as_float` explicitly
rounds to binary64 using locale-independent Rust conversion; finite subnormals are accepted, while
nonfinite overflow and mathematically nonzero values rounded to zero return
code8. Negative zero retains its sign. No process-global locale changes or fault state
are introduced.

Encoding is compact, preserves object order and number spelling, and emits UTF-8
with JSON escapes for controls, quotes and backslashes. It is not a canonical
signing format. It allocates once using validated subtree byte-count metadata.

## Stable errors

Parse offsets are zero-based input bytes; unexpected EOF uses input length.
Accessor/conversion/encoding failures use offset `-1`. Messages are static and
do not incorporate untrusted input. The first failure is preserved.

| Code | Name | Message |
| --- | --- | --- |
| 1 | Syntax | `invalid JSON syntax` |
| 2 | InvalidUnicode | `invalid JSON Unicode` |
| 3 | DuplicateKey | `duplicate JSON object key` |
| 4 | LimitExceeded | `JSON resource limit exceeded` |
| 5 | TypeMismatch | `JSON value has wrong type` |
| 6 | MissingKey | `JSON object key not found` |
| 7 | IndexOutOfBounds | `JSON array index out of bounds` |
| 8 | NumberOutOfRange | `JSON number out of range` |
| 9 | NonIntegralNumber | `JSON number is not an integer` |
| 10 | UnrepresentableString | `JSON string contains NUL` |
| 11 | NonFiniteNumber | `JSON number is not finite` |
| 12 | UnknownField | `unknown JSON object field` |

Code12 belongs to strict [typed record decoding](JSON_TYPED_CODECS.md); existing
dynamic operations keep their codes and empty error paths.

Code11 is used by the Float builder for NaN/infinity. The parser rejects those
spellings as syntax; numeric conversion overflow uses code8. Actual allocation exhaustion follows
the existing runtime allocator contract, not a promised recoverable JSON OOM.

## Bounds and testing

- Input: 1 MiB excluding the C terminator; over-limit offset is 1048576.
- Depth: 128, counting the root at depth1.
- Nodes: 100,000, counting root, children and object key String nodes.
- Logical allocations per parse: 32 MiB, including abandoned growth buffers and
  both sorting indices. Allocator bookkeeping and the fixed Result/error envelope
  are outside this logical byte count. Every allocation is charged before use.
- Encoded content: 16 MiB, excluding the terminator. Subtree metadata records
  height, expanded node count and encoded bytes for later immutable builders.
- Work: `8 * input_bytes + 64 * 100000`. The linear scan/copy bound of
  `8 * input_bytes` is reserved before parsing; additional node work, string
  decoding units, vector copies, merge steps and compared key bytes are charged
  from the remainder. Exhaustion returns code4, never a partial value.

Index construction uses bounded bottom-up merge sort, with compared bytes
charged individually to prevent common-prefix amplification. Lookup is bounded
binary search over at most 100,000 nodes and keys of at most1 MiB. Decimal
conversion loops are bounded by the number lexeme length and at most19 output
digits. Encoding traversal is bounded by cached expanded nodes/depth/output;
opaque J1 values cannot contain cycles or shared exponential trees.

The 32 MiB allocation and 16 MiB output checks are defensive ceilings: parsed
J1 trees normally reach the stricter input/node limits first. Internal boundary
tests exercise those ceilings directly without exposing a public limit override.
Builders enforce cached depth, expanded-node and encoded-byte metadata when
sealing; the parser also enforces depth/nodes during descent/allocation.

The shared `fern-json` crate implements parsing, indexing, conversion and
encoding for both native execution and the REPL. Runtime integration tests
exercise opaque ownership, malformed byte input and the full-width Result ABI.
The Rust collector has separate root, pressure, cycle and finalizer tests.

```sh
cargo test -p fern-json
cargo test -p fern-runtime --test json_values
cargo test -p fern-runtime --release
cargo xtask native json
```

## J2 builders and collection ABI

All added entry points retain the `fern_json_value_` prefix. `null`, `from_bool`
and `from_int` return a raw opaque Value pointer. `from_float(double)`,
`from_string`, `from_number_text`, `from_array(const FernList*)` and
`from_object(const FernList* keys, const FernList* values)` return heap Results.
The native object builder receives parallel String-pointer/Value-pointer lists.
It copies key bytes, validates Unicode, retains immutable child values, seals
expanded metadata and rejects duplicate decoded names. All builder errors have
offset-1. Every input list dimension is checked before reading its data.

`elements` returns Result(List(Value*), Error*). `members` returns
Result(List(FernJsonMember*), Error*); each member record has exactly two64-bit
fields, `key` at offset0 and `value` at offset8. Rust ABI tests and compiler adapter tests check this
layout. The Rust emitter converts successful member results to tagged three-word
source tuples; it preserves Err pointers without reading them as lists.
`limit_error()` is an internal adapter preflight helper, never a source function.

Scalar input text is limited to1 MiB. Object key scans stop once aggregate copied
bytes exceed16 MiB. Builder work reserves eight times newly scanned text bytes,
then charges repeated indexing comparisons against64 *100,000 units. Expanded
child metadata is checked without traversing shared subtrees. Existing storage is
not mutated. A maximum16 MiB encoded value may exceed the1 MiB parse-input cap.

Rust engine/runtime/source tests cover builder and collection behavior, large
encodings, depth/expanded-node limits, invalid Unicode, locale-independent
numbers and immutable copies. Historical C sanitizer counts are not Rust
acceptance counts; see [workspace acceptance](RUST_WORKSPACE.md).
