# Compiler tooling contracts

| Public surface | Rust implementation | Verification |
| --- | --- | --- |
| --version / -v, public help | Exact fern 0.1.0 identity, fern examples | cli_migration + cli |
| --quiet / --verbose / --color | Existing global controls retained | existing global control fixtures |
| build / run / check / emit | Existing independent frontend/native commands; literal -- source supported | cli_migration; native execution gates |
| lex / parse | Existing independent source debug commands; representation intentionally compiler-specific | cli_migration + source-only syntax tests |
| fmt | Existing safe source/directory canonical formatting, comments, --check | existing format CLI suites + literal filename regression |
| doc [path], --html / --open | Existing Markdown/HTML and opener; no-path now current project; inferred mode retained | documentation_cli + cli_migration |
| test, test --doc | Source-owned unit/docs execution; source path defaults cwd | existing native doctest/unit suites + literal source parse regression |
| repl evaluation + persistence | Existing typed evaluator, successful values retained without effect replay | repl suites |
| repl :type/:t/:h/:clear | Implemented and tested | tests/repl.rs |
| repl line editing/history/completion | Rustyline18.0.1, bounded history, real PTY parity, Ctrl-C recovery | crates/fern/tests/repl_terminal.rs + terminal history unit |
| lsp diagnostics/hover/completion/definition/formatting/sync | Existing module-aware Rust server incl unsaved buffers and UTF16 | existing LSP suites |
| lsp rename/codeAction | Checked local binding rename and real canonical-format edits; versioned UTF-16 changes | 18 action/refactoring/negotiation regressions, 65 existing LSP checks and executable RPC smoke |

### CLI and interactive tooling compatibility

The installed compiler identifies itself as `fern <release-version>`, matching the Cargo workspace version. Help examples use fern, and no banner labels the default compiler an experimental subset. Every source-taking command accepts `--` before a literal source filename, including names that match global options. Existing `fern run source.fn -- arguments` forwarding remains available. Documentation with no operand defaults to the current directory.

The REPL supports `:type` / `:t` without evaluating the inspected expression or replaying retained bindings. It obtains finalized inferred schemes from the ordinary checker, including native-only API result types. `:h` and `:clear` match the existing public command aliases. Piped and terminal REPLs share one bounded entry/paste state machine; Ctrl-C cancels the current terminal line/block/paste while preserving successful earlier definitions and values.

Terminal editing uses exactly pinned MIT-licensed Rustyline 18.0.1 with default features disabled and only with-file-history enabled. Its upstream supports Unix (Linux/macOS/FreeBSD) and Windows consoles; Fern's PTY regression gate covers the actual packaged Unix targets. Fern introduces no unsafe code or C frontend dependency for editing. Arrow/Delete editing, Tab completion using keywords plus the compiler's runtime API registry, and persistent history restore the C linenoise surface. History defaults to ~/.fern_history, accepts FERN_REPL_HISTORY override, reads legacy C plaintext and Rustyline v2 escapes, and ignores unavailable or nonregular history. Import reads at most 8 MiB plus one sentinel, retains at most 1000 entries with 4096 decoded bytes each, and ignores longer records. Submitted REPL entries retain the existing 1 MiB limit; Rustyline owns the transient line-editing buffer before submission. Nonterminal sessions emit neither prompts nor terminal escapes except explicit :clear.

Evidence: tests/cli_migration.rs pins public version, literal operands across all nine source-taking commands, extra-operand rejection and default project docs; tests/repl.rs covers effect-free type inspection and aliases; terminal module unit test covers bounded legacy/v2 history decoding; crates/fern/tests/repl_terminal.rs runs real PTYs for editing, completion, persistence across child processes, cancellation and nonregular history. All new behavior was tested red before implementation. Existing CLI/docs/REPL suites and private all-targets clippy pass.

Dependency source: Cargo.lock and downloaded crate source at rustyline18.0.1; upstream documentation https://github.com/kkawakam/rustyline (read 2026-09-12). `fern test` executes source-owned unit tests and executable documentation directly.

## Editor edit contract

Rust migration editor evidence: implement textDocument/prepareRename and rename with current-source lexical binding identities, checked before and after proposed edits (including identities for the new name so same-type captures are rejected). Return versioned UTF-16 WorkspaceEdit without mutating buffers or disk. Supported scope is current-document local bindings, private functions/clauses, and private aliases. Reject exported declarations/default parameter labels, imports, members, labels requiring interface-wide edits, invalid checked graphs, and ambiguous source namespaces; broad workspace refactoring is explicitly outside this bounded local operation. Work bounded by 512 matching old/new identifiers, 2 MiB source graph, 32 MiB reference indexing work, and 128-byte new names. Unsaved dependency overlays are honored.

textDocument/codeAction now returns an actionable source.fixAll.fern canonical formatting WorkspaceEdit, respects context.only, and offers nothing for already canonical or malformed syntax. This produces an edit clients can apply; no diagnostic repair is promised.

Evidence: 18 new tests (all green) cover Unicode/UTF16, declarations, patterns, closures, shadows, recursion/clauses, aliases, interpolation vs literal text/members/comments, both capture directions, reserved/invalid names, public parameter labels, unsaved module overrides, external targets, read-only/versioned edits, protocol routing/capabilities/errors, reference bounds, canonical/no-op/malformed/range/filter action behavior. 65 existing LSP tests across lsp, lsp_navigation, lsp_formatting, lsp_labels, lsp_aliases, lsp_namespaces, lsp_label_modules passed. Focused library clippy -D warnings green.

Versioned semantic edits require the client to advertise
`workspace.workspaceEdit.documentChanges`. Prepare-rename additionally requires
`textDocument.rename.prepareSupport`; literal code actions require their declared
`codeActionLiteralSupport`. Missing or malformed capabilities disable the
corresponding provider and unsupported requests fail explicitly. Older clients
retain diagnostics, navigation and ordinary document formatting.
