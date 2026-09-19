+++
schema_version = 1
id = "01M2XHZ80HMDDQP0NVQ0NHSMGM"
title = "Finish the language guides and support richer documentation markup"
date = "2026-09-15"
status = "accepted"
tags = ["docs"]
supersedes = []
superseded_by = []
depends_on = []
related_to = ["01M2XHZ815XA58AZ7ZM9368YT1"]
+++
## Status

Adopted; supersedes the Markdown dialect limits of [Decision 154](2026-09-14_192327141_generate-hexdocs-style-documentation-sites-from-moduledoc-do.md); verification is tracked in ROADMAP.md

## Decision

Complete the five introductory language chapters and include `docs/language/` in the repository documentation build. Extend the bounded Markdown renderer with images, reference links, footnotes, task lists, alerts, setext headings, strikethrough and allowlisted raw HTML. Validate rewritten URLs as well as source URLs; unsafe rewrites fall back to the original safe destination. Retain independent safety and output oracles.

## Context

The pending guide and renderer work contained a stale image assertion, links to unwritten chapters and several outdated language claims. Image support is also needed to display the project logo in the README and generated site.

## Consequences

Raw HTML is filtered and balanced, not unrestricted; executable schemes, event handlers and inline styles remain excluded. The repository xtask copies its local `docs/assets/` with the existing bounded, symlink-skipping copy helper. General compiler documentation does not fetch remote images. The site theme styles the new constructs: alerts reuse existing theme accents plus one new `--morrow-danger` variable, and task lists, footnotes, strikethrough and images gain rules so no construct renders as undifferentiated prose. Task checkboxes are drawn from theme colours rather than left to the browser, whose disabled-control styling made checked and unchecked items hard to tell apart. Image support is a renderer capability rather than a commitment to any particular artwork; the identity itself is the sunrise logo recorded in `docs/assets/README.md`.
