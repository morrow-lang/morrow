+++
schema_version = 1
id = "01M2XHZ8W49JDF009D9XG2Y1FA"
title = "Relocatable compiler bundles and isolated run artifacts"
date = "2026-09-05"
status = "accepted"
tags = ["runtime"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will install the runtime archive beside the compiler, resolve the actual executable location for runtime lookup, quote filesystem paths passed to the system toolchain, and create a private temporary directory for each `fern run` invocation.

## Context

Installing only the compiler and resolving argv[0] failed outside the checkout; unquoted paths broke ordinary directory names; predictable run paths could overwrite unrelated files. The `/decision` skill is unavailable in this checkout/session, so this entry follows the existing decision format directly.

## Consequences

Bundles remain relocatable, `PREFIX` supports local installation, and simultaneous runs have separate artifacts. Native compilation still requires the documented host compiler and libraries.
