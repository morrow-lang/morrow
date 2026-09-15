# Executable language compatibility

The Rust rewrite preserves the executable Morrow language established before the
migration. It does not implement every future feature in `DESIGN.md`. Full-width
values, checked Result obligations and explicit faults remain required; known
miscompilation is not a compatibility oracle.

| Surface | Current contract | Evidence |
| --- | --- | --- |
| Builtin names | The 212 names inventoried from the prior implementation have a Rust registry or compiler-intrinsic contract. | Runtime registry and ABI suites |
| Compatibility modules | Http, Sql and Actors share identities with lowercase service names; File/fs and Json/json retain their documented aliases. | Registry signatures and native `migration/aliases.mr` |
| Indentation | Consistent tabs use eight-column stops; mixed significant indentation rejects. Formatting emits spaces. | Parser/formatter suites and seeded corpus |
| Inline matches | Comma-separated value arms retain typed patterns, guards and nearest-unclosed-match grouping. | Parser/checker/formatter/REPL tests and native `migration/inline_match.mr` |
| Bracket indexing | `items[index]` shares `List.get`, including nested access, full-width transport and faults. Formatting canonicalizes it to `List.get`. | Native `migration/indexing.mr` and rejection fixtures |
| Membership | Int, Float, Bool and String comparisons evaluate both operands once in source order. | Native `migration/membership.mr` and IEEE/effect tests |
| Results and cleanup | Indexing does not discharge unvisited Result elements; rejection and faults preserve output and cleanup contracts. | Result proof tests, atomic rejection checks and `migration/index_fault.mr` |
| Native ABI | Registered symbols have typed transports/adapters; Cranelift links the Rust runtime. | Object/ABI tests and independent native expected-output fixtures |
| JSON | Opaque values/errors, immutable builders and typed codecs replace the retired String-copy API. | JSON engine, runtime, source and REPL suites |

```sh
cargo test -p morrow --test migration_language
cargo xtask build
cargo xtask native migration/
cargo xtask compatibility
```

`cargo xtask native` executes 305 independent expected-output fixtures. The
compatibility command preserves additional dynamic applications, atomic rejection
checks and continued execution of later tests after a failed unit test. These
Rust runners retain literal subprocess arguments and use the Rust supervisor for
bounded native execution; they do not require an installed reference compiler.

General traits, list comprehensions, Map bracket indexing, Sets, HTTP serving,
typed database queries, generalized actor supervision, advanced ownership
analysis and WASM remain outside the accepted implementation unless their own
roadmap gates say otherwise. The [workspace acceptance](RUST_WORKSPACE.md) records
current platform evidence; [release readiness](RELEASE_READINESS.md) records
remaining work.
