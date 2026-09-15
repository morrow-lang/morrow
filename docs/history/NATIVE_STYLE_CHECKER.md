> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

> Historical document, retained for architectural context. Its implementation paths, commands and measurements describe the retired setup. See [the current build guide](../../BUILD.md) and [roadmap](../../ROADMAP.md).

# Native quality-checker launcher

`mise run style`, `mise run style-lenient`, `mise run pre-commit` and the primary checker in
`mise run check` use `scripts/check_style`. It builds and caches the Fern checker with
the C bootstrap compiler. The native checker process needs no Python or Cargo. Mise may still provision
its configured project tools; direct launcher execution needs only native tools.
The full `mise run check` gate still runs explicit Python integration tests; Python
3.14 remains the independent diagnostic and workflow reference.

The launcher requires Bash 3.2 or newer, Clang 14 or newer (including Apple Clang),
OpenSSL, pkg-config and the native libraries listed in BUILD.md. GCC remains
supported by the ordinary C build but is outside this launcher's private compiler
profile. Builds use literal argument arrays. Compiler-owned empty configuration
and a compiler-scoped environment setting suppress implicit Clang configuration;
caller config/plugin/response/opaque forwarding flags and nonempty
`CCC_OVERRIDE_OPTIONS` reject explicitly. Trusted wrappers must obey this profile.

## Cache and configuration

The default cache is `${XDG_CACHE_HOME:-$HOME/.cache}/fern-style`.
An unset or empty `XDG_CACHE_HOME` uses the same HOME fallback.
`FERN_STYLE_CACHE` selects another private directory. `FERN_STYLE_TOOL_PATH`
selects bootstrap tools separately from the PATH restored to the final checker;
`FERN_STYLE_CC` selects a trusted Clang-compatible compiler or wrapper.
`scripts/clean_style_cache` retires complete owned bundles. It takes no arguments.

Cache hits verify source, tool, runtime and external dependency contents plus
compiler/library lookup directories. All authored runtime inputs participate,
including `.inc` files. Paths and metadata are bounded literal data, never shell
code. Files whose ownership, type or permissions violate the private-cache
contract reject. Timestamps alone never establish freshness. Warm hits invoke no
compiler; failed builds never fall back to stale executables.

A build uses an immutable source snapshot and checks freshness before publication.
One source change triggers a fresh retry; a second fails. Publication needs no PID
lock or lock stealing. The cache retains at most eight complete entries with a
64-entry scan bound. Each active invocation retains its executables independently,
so pruning cannot remove the running program. Incomplete work and active runs are
never guessed to be abandoned or deleted by another invocation.

The snapshot permits 4,096 source files and 64 MiB total. External dependencies
permit 16,384 paths. Lookup inventories permit 128 roots, 131,072 entries, 128
levels and 32 MiB of records. Ancestor symlink cycles record the link and stop
re-expansion. If directory listing returns `EACCES`, the inventory records an explicit
`U` marker only when `faccessat(..., X_OK, AT_EACCESS)` also reports `EACCES` for the
effective user. Such a user cannot resolve descendant headers or libraries. Every
cache lookup repeats this check; gained search access changes the inventory and
invalidates the cache. A directory that denies listing but permits search still
rejects: a compiler could open a known filename that the inventory cannot enumerate.
Other access errors retain their existing failure behavior. Paths containing newline
or carriage return reject. Cache freshness
is checked at lookup and publication boundaries; it is not a filesystem-wide
transaction against concurrent hostile mutation or a trace of arbitrary wrapper
configuration.

## Execution and failures

Native checker exits and stdout/stderr retain their established behavior.
Bootstrap or supervision failures exit 125 with a best-effort diagnostic.
The launcher restores caller PATH, locale, umask and standard-descriptor state
before checker execution. The cold build worker separately sets `umask 077` for its
private generated metadata and artifacts, including when the caller uses `umask 002`.
This child-only setting does not change the final checker's caller mask. Unrelated
inherited descriptors are closed, including
ones above a lowered soft limit. Compiler configuration overrides remain scoped
to compiler children.

A native controller owns each private build process group and retains the direct
child identity until group cleanup, then reaps it. Bash job IDs never establish
process ownership. A four-line bounded handoff is checked against held directory
and executable identities before entering inherited-stdio checker execution.
A worker's exit zero alone is insufficient. The final checker has no build-time
or build-output cap.

After the controller exists, builds have a 120-second deadline and 16 MiB per
captured stream. The initial trusted helper compilation runs in the foreground
with EOF stdin, private logs, CPU and file-size limits; it has no hard wall-clock
guarantee before the native helper exists. Deadlines initiate cleanup; kernel
reaping or blocking output can extend elapsed time. Descendants that escape their
private process group are outside containment. Normal failures remove their own
staging; forced termination may leave private incomplete work that is never reused.

## Verification

Native/Python parity covers 66 workflow cases plus exact style diagnostics over
pinned failing fixtures and compiler/library source. macOS arm64 and Linux arm64
also verify cold/warm lookup, content and external-header changes, concurrent
invocations, retry limits, no-Python execution, config injection, retained-file
cleanup, writerless FIFO rejection and explicit cache pruning. Supervisor,
controller, descriptor and directory-inventory matrices run in debug, release,
AddressSanitizer and UndefinedBehaviorSanitizer profiles.

The recipe regression executes mise with Python tools that fail if invoked. It
checks literal native arguments, streams and exit propagation for style, lenient,
pre-commit and early-failing full checks. `mise run style-launcher-check` runs the
launcher infrastructure suite; these verification tools intentionally use Python.
