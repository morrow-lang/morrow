+++
schema_version = 1
id = "01M2XHZ8A5C2MYJ3HH91NTDVYX"
title = "Complete CLI and interactive-tool compatibility with safe Rust"
date = "2026-09-12"
status = "accepted"
tags = ["rust", "tooling"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted; compiler/tooling platform acceptance verified
* **Decision**: I will preserve public fern identity, literal source operands, default documentation discovery and REPL inspection/editing behavior in the Rust default. Use exactly pinned Rustyline18.0.1 with only file-history support enabled for terminal editing.
* **Context**: Failing compatibility and real-PTY tests demonstrated missing command delimiters, inspection commands, editing, completion and persistent history. Standard safe Rust does not supply a readline terminal editor; a maintained dependency avoids adding custom unsafe terminal control.
* **Consequences**: Piped execution keeps its existing bounded state machine and quiet output. Type inspection has no runtime effects, Ctrl-C cancels pending input without losing prior state, and history import has explicit byte/entry limits and rejects nonregular inputs. Package the dependency notices. The C developer shell-command test overrides remain explicit bootstrap-only facilities; Rust executes source tests directly. Semantic LSP actions publish real versioned edits, reject ambiguous bindings and negotiate client support for documentChanges, prepareRename and literal actions. See docs/TOOLING_PARITY.md for the tested command matrix and limits.
