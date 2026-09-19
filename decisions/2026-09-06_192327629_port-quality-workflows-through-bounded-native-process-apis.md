+++
schema_version = 1
id = "01M2XHZ8GD7ZXEV17W5A8SWP3S"
title = "Port quality workflows through bounded native process APIs"
date = "2026-09-06"
status = "accepted"
tags = ["ci"]
supersedes = []
superseded_by = []
depends_on = ["01M2XHZ8FPJ1H8EQ4H7FFQG8YW"]
related_to = []
+++
## Status

Accepted for the bootstrap workflow checkpoint; default migration remains open

## Decision

I will implement the Fern checker with immutable returned state, literal `System.exec_args_bounded` commands, separate diagnostic stderr, and source-level handling of process failures. Compare exact style diagnostics and semantic command workflows against Python before changing the default.

## Context

Style-only parity did not cover build/test/example continuation, Git checks, command arguments or CLI failures. The shared process and stderr APIs now provide the required native contracts without a shell or lossy text capture.

## Consequences

Every command has a 300-second deadline and independent 8 MiB stream limits. Ordinary tool output and argument order remain tested; OS-specific exception wording and terminal decoration are not byte-identical contracts. CLI write failures preserve exit 2. Workflow oracles pin Python 3.14 because argparse short-help clustering changed since Python 3.11; native CLI behavior is fixed across hosts. [The checker contract](../docs/history/BOOTSTRAP_CHECKER.md) records 47 workflow scenarios and the remaining Unicode numeric-path classification gap. Python remains the reference and shipping default until remaining parity and launch gates pass. The unavailable `/decision` skill is replaced by this established format.

The [Decision 92](2026-09-06_192327606_pin-decimal-text-classification-to-unicode-16-with-bounded-w.md) follow-on closes the argument-classification gap: only the first
scalar after `-` or `-.` is classified, matching Python 3.14 prefix semantics.
Nineteen additional reference-first scenarios bring workflow coverage to 66 cases
under both frontends. Native default launch and cache correctness remain separate.
