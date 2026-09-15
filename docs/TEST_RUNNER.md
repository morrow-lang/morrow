# Rust unit tests and documentation examples

`morrow test` runs ordinary `test_` functions and documentation examples in the
current directory. Supply a source file or directory to select a smaller suite.
`--doc` selects only documentation examples, including when ordinary helper
functions happen to use the test naming convention.

```morrow
fn add(left: Int, right: Int) -> Int: left + right

fn test_add() -> Result((), Int):
    if add(2, 3) == 5: Ok(())
    else: Err(1)
```

Each unit test must take zero arguments and return Unit or Result(Unit,E). A normal
Unit return or Ok passes; Err and runtime faults fail. Boolean and integer results
are rejected rather than silently treated as assertions. Generic tests are rejected
from their reusable source signatures, even if another function happens to demand
a concrete specialization. Helpers may be generic and take ordinary arguments.

Tests run in independent native processes. Private helpers and imports work as in
the owning source file. Original main functions remain callable; selecting a test
does not replace resolved calls to main. Only functions actually declared in each
selected source count as its unit tests, so imports are not accidentally rerun.
Adjacent function clauses form one test. Failing test signatures and runtime failures
are reported with the original source path, line and test name; later tests continue.
Malformed source and discovery-limit violations stop discovery with a diagnostic.

Invoking System.exit during a unit or documentation test always produces a diagnostic
and a failing process exit, including through helpers or function values. It cannot
bypass remaining assertions with exit 0. Ordinary application compilation is unchanged,
and unused application exit functions do not invalidate tests. Test process-exit
behavior by running a child process and checking its returned status.

The runner executes user code and requires the native backend/runtime dependencies.
Each process has closed stdin, a default 10-second timeout (`--timeout 1` through
`--timeout 60`), and 256 KiB limits per output
stream. Unix process groups are cleaned up after completion, failure or timeout.
Discovery allows 256 files, 8 MiB aggregate source, 1 MiB per file and a combined 256
unit/documentation tests. Documentation blocks retain their 64 KiB limit. A zero-test
run reports 0/0 explicitly. Source files are never rewritten.

Assertion libraries and structural assertion messages, benchmarks, coverage and
watch mode remain separate implementation work. Unsupported options are rejected.
Documentation expectations continue to use ordinary checked patterns after `# =>`.
See [the Rust frontend guide](../crates/morrow/README.md) for documentation details.
