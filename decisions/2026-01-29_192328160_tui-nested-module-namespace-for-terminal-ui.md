+++
schema_version = 1
id = "01M2XHZ910K4S1RW2K7YJCW57T"
title = "Tui.* nested module namespace for terminal UI"
date = "2026-01-29"
status = "accepted"
tags = ["shell"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Adopted
* **Decision**: I will organize all terminal UI modules under a `Tui.*` namespace (e.g., `Tui.Panel`, `Tui.Table`, `Tui.Style`) instead of using flat top-level module names.
* **Context**: The original flat module names (`Panel`, `Table`, `Style`, `Status`, `Live`, `Progress`, `Spinner`, `Term`) were ambiguous - `Style` could mean anything, `Panel`/`Table` aren't clearly TUI-related at a glance, and `Term` (terminal capabilities) belongs with other TUI modules. Considered two approaches: (1) Keep flat names - simple but poor organization and discoverability, (2) Nested `Tui.*` namespace - groups related functionality, follows Elixir's convention (e.g., `Phoenix.HTML`, `Ecto.Query`), makes it clear these are terminal UI modules. The nested approach also prepares the language for future namespace organization (e.g., `Http.*`, `Json.*`, `Crypto.*`). Implemented proper nested module support using `try_build_module_path()` helper that recursively builds paths from dot expressions, enabling arbitrary nesting depth.
* **Consequences**: All TUI modules renamed: `Term` → `Tui.Term`, `Panel` → `Tui.Panel`, `Table` → `Tui.Table`, `Style` → `Tui.Style`, `Status` → `Tui.Status`, `Live` → `Tui.Live`, `Progress` → `Tui.Progress`, `Spinner` → `Tui.Spinner`. Both checker.c and codegen.c updated to recognize nested module paths. Examples updated to use new namespace. The module system now supports arbitrary nesting (e.g., `Tui.Style.Colors` would work if implemented). DESIGN.md updated with comprehensive Tui module documentation.
