+++
schema_version = 1
id = "01M2XHZ97B4V992VGPE1PRM59K"
title = "Python-style indentation syntax"
date = "2026-01-26"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will use significant indentation (Python-style) instead of braces or `end` keywords.

## Context

Readability is a primary goal. Compared options: (1) Braces `{}` - familiar but add visual noise, (2) `end` keywords - clear but verbose, (3) Significant whitespace - clean and minimal. Python proves indentation works at scale. Modern editors handle indentation well. The reduced visual noise improves readability significantly.

## Consequences

The lexer must track indentation levels and emit INDENT/DEDENT tokens. Mixed tabs/spaces must be rejected. Error messages must handle indentation errors clearly.
