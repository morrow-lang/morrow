# Executable language migration parity

The migration acceptance baseline is the shipping C compiler's executable
language, not every future feature in DESIGN.md. Rust keeps its stricter Result
handling, explicit faults, full-width values, and typed native boundaries rather
than preserving known C miscompilation.

| Surface | Audit outcome | Evidence |
| --- | --- | --- |
| Canonical builtin names | All 212 names extracted from C module/function and bare-builtin registration have a Rust registry or compiler-intrinsic contract. | `all_shipping_c_builtin_names_have_a_rust_registry_or_intrinsic_contract` |
| Compatibility modules | Http, Sql, and Actors expose the same 14 existing service operations as their lowercase forms, sharing identities and ABIs. File/fs and Json/json retain their existing contracts. | Registry identity/signature test and native `aliases.fn` |
| Indentation | Consistent tabs use eight-column tab stops; mixed significant indentation rejects. Formatter emits canonical spaces and byte spans remain source-based. | Parser/layout/formatter regressions and original seeded parser/formatter corpus |
| Inline match arms | Comma-separated value-match arms share typed patterns and guards; nested arms bind to the nearest unclosed match. Group the match before a following caller lambda. | Seven bounded parser/checker/formatter/REPL tests, native `inline_match.fn` and unchanged seeded corpus |
| Bracket list indexing | Restored `items[index]`, chained indexing, tuple selection, and typed full-width element transport. Parsing lowers to the existing `List.get` operation. | Parser/checker/format/REPL tests; native `indexing.fn` |
| Membership | Restored comparison-precedence `item in items` for Int, Float, Bool, and String elements, including generic scalar functions. Both operands evaluate exactly once in source order. | Native `membership.fn`; source-order/precedence/IEEE tests |
| Bounds and Result handling | Indexing uses the same native fault/defer path and Result-use rules as `List.get`; indexing does not prove that unvisited Result elements were handled. | Invalid/type/depth tests; native `index_fault.fn` |
| Existing native ABI | Every registered symbol has a declared signature and explicit transport or adapter; compiler-owned and unsafe legacy helpers remain inventoried separately. | `runtime`, `qbe_runtime`, and `runtime_abi` suites |
| C executable reference subset | Service aliases, scalar list indexing and membership preserve exact C/native output for valid reference sources. | `reference.fn`, `aliases.fn` and `--reference-compiler` gate |
| C AST-only constructs | List comprehensions, general traits, and Map bracket indexing are not migration requirements merely because C parses or typechecks them. C lacks corresponding complete executable lowering (Map indexing incorrectly uses list layout). | C AST/checker/codegen inspection |
| Separate compatibility changes | Rust opaque JSON replaces C's legacy String-copy contract; Rust keeps lossless Options and explicit collection/numeric faults. | Decisions 95–103 and existing native/ABI suites |

The formatter canonicalizes bracket indexing to `List.get`. This makes indexing
share the language's existing type inference, optional-value, Result, and cleanup
semantics without introducing a second runtime operation. A list expression after
an indented suite remains a separate statement; it is not treated as indexing the
preceding loop or conditional.

```sh
cargo test --manifest-path compiler-rs/Cargo.toml --locked --test migration_language
python3 scripts/test_migration_language.py --compiler bin/fern-rs --reference-compiler bin/fern-c
```

The native gate independently specifies language and recursive Result-builder
programs' output, including cleanup before an invalid-index fault. `--backend cranelift` uses the same oracles with the
optional backend. The compiler paths are explicit so the gate remains usable
before and after default-command migration.

This matrix complements the complete Rust/native gates. It does not claim that
future database queries, HTTP serving, Sets, generalized actor supervision,
custom traits, ownership analysis, or WASM are implemented. Command tooling,
packaging, platform validation, actor execution and recursive Result proofs have
separate migration acceptance evidence.
