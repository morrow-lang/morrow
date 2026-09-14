# Native foreign functions

Fern can call C-compatible functions implemented in Rust or third-party native
libraries. Foreign declarations are a **trusted boundary**: the author promises
that the symbol, exact ABI, pointer validity and ownership contract are correct.
The compiler checks transport types; it cannot verify another library's memory.
Prefer a native Rust library and a small Rust `extern "C"` adapter where possible.

```fern
foreign "C" fn absolute(value: Int) -> Int as "llabs"
foreign "C" fn cosine(value: Float) -> Float as "cos" from "m"

fn main():
    println(absolute(-42))
    println(cosine(0.0))
```

`as` chooses the linker symbol; without it the declared function name is used.
`from` chooses a logical library name (`m` becomes the separate linker argument
`-lm`). Paths, flags and punctuation are rejected. Libraries must be installed
for the selected native target. Foreign libraries may introduce deployment
requirements; they are not automatically bundled into a standalone binary.
Ordinary visibility, imports, argument labels and function values apply.

## Exact scalar types

| Fern type | C ABI |
| --- | --- |
| `Int` | `int64_t` |
| `Float` | `double` |
| `Bool` | C `_Bool` / Rust `bool` |
| `()` | `void` return only |
| `CInt8`, `CInt16`, `CInt32` | signed integer of the named width |
| `CUInt8`, `CUInt16`, `CUInt32`, `CUInt64` | unsigned integer of the named width |
| `CFloat32` | `float` |
| `Ptr(a)` | pointer with a checked, opaque pointee identity |

Narrow scalars are distinct compiler-owned types. `CInt32.from_int(value)` and
other integer constructors return `Result(CInt32, String)` (with the respective
type), rejecting values outside the representable range. `.to_int(value)` returns
`Int`, except `CUInt64.to_int`, which returns `Result(Int, String)` because Fern
Int cannot represent the upper half of unsigned 64-bit values. Foreign-returned
`CUInt64` values preserve all 64 bits and can be passed back unchanged.

`CFloat32.from_float` returns `Result(CFloat32, String)`, rounds to binary32 at the
conversion and rejects finite overflow. NaN, infinities and signed zero are
preserved. `CFloat32.to_float` expands the rounded value to Fern Float. Normal
Result obligations apply to these fallible conversions.

No implicit `Int`-to-C-int narrowing occurs. Aggregates, generic foreign
signatures, variadic functions, callbacks, C structs passed by value and raw
`*mut` / `*const` syntax are outside this boundary. A Rust adapter can translate
these interfaces into explicit scalars and opaque handles.

## Opaque pointers and strings

`Ptr(a)` cannot be constructed from an integer, inspected, updated as a record,
dereferenced, or used in pointer arithmetic. Pointee types distinguish unrelated
handles. `Ptr.null()` needs context for its pointee type; `Ptr.is_null(pointer)`
and `Ptr.equal(left, right)` inspect nullness and same-type address identity.
Pointers cannot cross actor mailboxes or be JSON encoded. Keep foreign pointers
inside synchronous wrappers: native actor continuation frames also reject them,
so a pointer cannot remain live across actor suspension.

```fern
foreign "C" fn suffix(value: Ptr(CUInt8)) -> Ptr(CUInt8) as "library_suffix"

fn read_suffix(text: String) -> Result(String, String):
    let pointer = suffix(text.as_ptr())
    Ptr.to_string(pointer, 4096)
```

`String.as_ptr(text)` (also `text.as_ptr()`) borrows an immutable UTF-8 C string
and retains its Fern owner in the sealed pointer. A foreign pointer result
conservatively retains the string owners of every pointer argument. Interior
pointers therefore remain valid after the original local string leaves scope.
The collector traces those retained owners. Embedded NUL follows Fern's existing
C-string boundary: foreign code sees the prefix before the first NUL.

`Ptr.to_string(Ptr(CUInt8), byte_limit)` copies into an owned Fern String and
returns `Result(String, String)`. Null, invalid UTF-8, a missing NUL within the
bound, and limits outside `1..=1048576` are errors. The limit includes the NUL.
For a non-null foreign-owned pointer, the caller must guarantee readable,
unchanging memory through the first NUL or the bound, whichever comes first.
A length check cannot prove a foreign address is valid.

Foreign code must not mutate or free borrowed Fern strings. It may retain their
addresses only while a corresponding Fern pointer remains alive. Foreign-owned
handles carry their library's allocation, sharing and destruction contract:
Fern does not automatically free them or prevent use after an explicitly called
foreign destructor. Wrap these functions behind a narrow, documented API.

## Targets and acceptance

Declarations can be checked and formatted on every host. Execution is native;
the REPL, browser WASM and compile-time evaluator reject foreign effects.
Pure checked scalar conversions and null-pointer inspection work in the REPL.
The compiler rejects incompatible duplicate symbols, reserved runtime symbols,
invalid parameter counts and malformed ABI metadata before object emission.

Acceptance includes real Rust `extern "C"` fixtures, 1,024 seeded exact-width
calls, mixed register/stack arguments, signed/unsigned extremes, Bool, binary32
rounding, pointers and void effects. Runtime conversion simulation checks 4,096
independent floating bit patterns. Source-to-native tests force precise garbage
collection before foreign calls and bounded reads, retaining a returned interior
UTF-8 pointer after its source function exits. Checker tests cover checked
conversions, private storage (including record updates), type identity, invalid
libraries, effects, formatting and target rejection.
