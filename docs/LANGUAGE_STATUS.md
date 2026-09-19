# Executable language status

Morrow is a pre-1.0 language with a Rust compiler, runtime and language server.
The [design](../DESIGN.md) includes future proposals. This page describes the
implemented language and links to its execution contracts.

| Feature | Native | REPL | Browser WASM |
| --- | --- | --- | --- |
| Checked functions, clauses, patterns, generics and labeled arguments | Yes | Yes | Yes |
| Records, tagged sums, tuples, aliases and unboxed newtypes | Yes | Yes | Yes |
| Explicit finite unions and typed narrowing | Yes | Yes | Yes |
| Closures, captured values and function arguments | Yes | Yes | Yes |
| Immutable Lists, Maps and Sets | Yes | Yes | Yes |
| Ranges, `for`, lexical exits, `with`, `?` and `defer` | Yes | Yes | Yes |
| Static traits, defaults, parents and explicit `where` bounds | Yes | Yes | Yes, with portable methods |
| Structural Show/Eq/Ord/Clone derivation | Yes | Yes | Yes |
| Pure compile-time constant data | Yes | Yes | Yes |
| Dynamic, derived and custom JSON codecs | Yes | Yes | Host capability not implemented |
| Typed actor mailboxes, suspension and supervision | Yes | Virtual-time scheduler | Server-side capability |
| Parallel pinned actor schedulers | Opt-in `MORROW_SCHEDULERS=1..64` | No; deterministic interactive scheduler | Server-side capability |
| Source C FFI and retained pointer handles | Yes | Rejected | Rejected |

“Yes” refers to the documented feature contract, not every possible future
extension. Sets use supported scalar keys. Unions require explicit representable
members; constructor refinements, implicit joins and lifted capabilities remain
proposals. Compile-time constants cannot reflect on types, create native resources
or run host effects. Foreign interfaces require exact trusted ABI declarations.
See [Sets](SETS.md), [comptime](COMPTIME.md), [traits](TRAITS.md), [FFI](FFI.md),
[custom JSON](CUSTOM_JSON.md) and [WASM](WASM_LANGUAGE.md) for details.

Actors use automatic memory management, immutable messages and typed continuation
frames. The [native scheduling contract](ACTOR_CONTINUATIONS.md) and
[interactive scheduler](REPL_ACTORS.md) explain suspension, cleanup, resource
limits and virtual time. These features do not establish Erlang-equivalent
supervision trees, instruction preemption or distributed recovery. The browser
runs a model/update/view application and communicates with server actors over the
existing WebSocket protocol.

## Try the language

```sh
cargo xtask build
bin/morrow run examples/language_tour.mr
bin/morrow repl
```

The tour combines a custom trait, structural derivation, a compile-time reduction,
Sets, full-width integer identifiers, JSON round trips and deferred cleanup.
Its exact native and interactive output is tested.

## Hardening evidence

The workspace gate runs strict formatting and Clippy, unit and integration tests,
native stdout fixtures, examples, process/tooling checks and bounded fuzzing:

```sh
cargo xtask check
cargo xtask fuzz 512 0xC0FFEE
cargo test -p morrow-sim --test language_actors
cargo xtask simulate --actors --seed 0xc0ffee --steps 5000 --json
cargo xtask simulate --seed 0xc0ffee --steps 3000 --days 30 --json
```

Independent models cover collection updates, derived comparison and cloning,
JSON schema intersections, full-width custom-codec round trips, virtual actor
mailboxes, restart identity and cleanup stacks. Native ABI tests inject precise
collection at callback and scheduling boundaries. Fuzzing preserves the original
192 mutation oracles and adds 231 current-language mutations per campaign.

Simulated days describe virtual schedules. They are not equivalent to years of
production exposure. Platform execution, real browser behavior, long-running
workloads and future distribution protocols need their own acceptance evidence.
The [roadmap](../ROADMAP.md) retains those open gates.
