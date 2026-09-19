+++
schema_version = 1
id = "01M2XHZ927K0XAG4KV7GTD0TAY"
title = "Built-in module syntax for standard functions"
date = "2026-01-29"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Adopted
* **Decision**: I will use `Module.function()` syntax for built-in functions instead of flat names like `str_len()`, organized into `String`, `List`, `File`, `Result`, and `Option` modules.
* **Context**: The original flat naming convention (`str_len`, `str_concat`, `list_get`, `file_read`) works but has discoverability issues. Users can't easily find what functions are available without memorizing prefixes. Considered two approaches: (1) Keep flat names - simple but poor discoverability, (2) Module-qualified syntax `String.len()` - familiar from many languages, enables LSP autocomplete on `String.`, groups related functions clearly. The module approach is foundational for the language to "feel right" and prepares for future LSP integration where typing `String.` shows all available string functions.
* **Consequences**: Added `is_builtin_module()` and `lookup_module_function()` in checker.c. Updated EXPR_DOT handling to recognize module access. Added codegen support for module.function calls. Old flat names still work for backwards compatibility. All examples updated to use new syntax. DESIGN.md documents the built-in modules. Future: deprecation warnings for old syntax, eventual removal.
