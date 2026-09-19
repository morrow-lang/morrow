+++
schema_version = 1
id = "01M2XHZ8J0B7HCZM1BQH3SE91S"
title = "Run source-owned unit tests alongside documentation examples"
date = "2026-09-06"
status = "accepted"
tags = ["testing", "docs"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for Rust migration completion
* **Decision**: I will discover zero-argument `test_` functions from parsed source and execute each checked Unit or Result(Unit,E) function independently. Normal `fern test` includes both unit tests and documentation examples; `--doc` restricts execution to documentation.
* **Context**: The design specifies ordinary named test functions, but the current command only executes documentation examples. Selecting by original source identity avoids rerunning imports or replacing user main references. Boolean/integer return values must not silently pass as unasserted tests.
* **Consequences**: Real private helpers and original main remain callable. Native tests use the existing bounded capture and cleanup mechanism, continue after failures (including invalid test signatures), and report original names and locations. Discovery retains parameterized groups so eligibility errors are reported independently for each test. Combined discovery is limited to 256 tests; unsupported result signatures reject before native compilation. Unit and documentation execution use a dedicated QBE test mode: invoking the resolved process-exit API always diagnoses and exits unsuccessfully, including through helpers and callbacks, so exit0 cannot bypass remaining assertions. Ordinary application emission and unused exit functions remain unchanged; exit-behavior tests must use a child process. Eligibility uses one ordinary checker pass and the selected reusable source signature, with no editor metadata budget. Assertion libraries, benchmarks, coverage and watch remain tracked separately. The unavailable `/decision` skill is replaced by the established decision format.
