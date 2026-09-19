+++
schema_version = 1
id = "01M2XHZ8ND1D8W2TSXKYFK417X"
title = "Generate source documentation with the Rust parser"
date = "2026-09-05"
status = "accepted"
tags = ["rust", "docs"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will generate Rust frontend documentation from parsed source declarations and literal @doc metadata, grouping function clauses and retaining their original source signatures. Documentation generation does not require an executable main or run examples implicitly.
* **Context**: The current Python generator recognizes signatures with a regular expression, which cannot cover nested function types, clause patterns or Unicode identifiers reliably. The Rust parser already establishes declaration boundaries and documentation ownership.
* **Consequences**: The first checkpoint accepts one source file, writes Markdown by default or standalone escaped HTML, and supports atomic output files without overwriting source aliases. Parsing and output are bounded. All declarations are included and public visibility is shown; inferred signatures are not invented from omitted annotations. Directory navigation/search and explicit executable doc tests follow as separate checkpoints. Documentation text is literal data in HTML; no scripts or remote assets are required. The unavailable `/decision` skill is replaced by the established decision format.
