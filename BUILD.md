# Building Fern Compiler

## Prerequisites

- Rust toolchain selected by mise (see the dated nightly below)
- C compiler (clang or gcc) for native components and linking
- mise 2026.9.1 or newer (CI pins 2026.9.1)
- Boehm GC development library (`bdw-gc`)
- SQLite development library (`sqlite3`)
- OpenSSL development library (`openssl`)
- `pkg-config` for native library discovery
- Clang 14+ and Bash 3.2+ for the native quality-checker launcher
- Python 3.11+ and `uv` for integration/reference tests and documentation tooling
- macOS, Linux, or other Unix-like OS

Install the native development dependencies:

```sh
# macOS (with Xcode command line tools installed)
brew install mise bdw-gc sqlite openssl pkg-config

# Ubuntu/Debian (install mise using its official installation instructions)
sudo apt-get install clang pkg-config libgc-dev libsqlite3-dev libssl-dev
```

The repository pins Rust **nightly-2026-09-06**, Python 3.14.7 and uv 0.12.5
in `mise.toml`. A matching root `rust-toolchain.toml` selects the same Rust
compiler for direct Cargo commands.
Run `mise install`, then `mise run tool-versions`. Review and trust this checkout
when mise requests it; no global configuration or activation hook is needed.
The native packages above are host-managed, not a fully pinned OS image.
See [the task and tool environment](docs/DEVELOPMENT_ENVIRONMENT.md) for lockfiles,
optional runners, nightly update policy and the remaining reproducibility boundary.

## Quick Start

```bash
# Build the compiler (debug mode)
mise run debug
# Run tests
mise run test

# Build release version
mise run release

# Clean build artifacts
mise run clean
```

## Build Targets

### Default Rust compiler

`mise run debug` and `mise run release` build the Rust compiler as `bin/fern`
and retain `bin/fern-rs` as a development alias. They also build the explicit
C reference compiler `bin/fern-c`, QBE helper `bin/fern-qbe`, native test supervisor
`bin/fern-test-supervisor`, runtime archive `bin/libfern_runtime.a`, and
`bin/fern-package.json` component marker.

```sh
mise run debug
./bin/fern run compiler-rs/tests/corpus/hello.fn
./bin/fern-c check examples/tiny_cli.fn  # Explicit C reference
mise run rust-check
```

`mise run rust-build` and `rust-release` remain compatibility tasks for compiler
development. `mise run rust-cranelift-build` produces the separate
`bin/fern-rs-cranelift`; select its optional backend with `--backend=cranelift`.
Ordinary `fern` builds use QBE. See the [compiler guide](compiler-rs/README.md)
for language and backend boundaries, and [migration progress](docs/RUST_MIGRATION.md)
for acceptance status.

### Development

- `mise run debug` - Build debug version with symbols and assertions
- `mise run test` - Build and run C reference, native runtime and default-command integration tests
- `mise run rust-check` - Check formatting, Clippy, Rust tests and native language oracles
- `mise run clean` - Remove native build outputs; Cargo retains its incremental cache

Full Rust/backend suites produce many test executables. On constrained machines,
use the same settings as CI: `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`
and `CARGO_PROFILE_TEST_DEBUG=0`. Keep Cargo targets and `TMPDIR` on a disk with
adequate space rather than a small RAM filesystem. `cargo clean --manifest-path
compiler-rs/Cargo.toml` clears generated Rust artifacts when needed.

### Production

- `mise run release` - Build optimized release version

### Installation

- `mise run install` - Install `fern`, `fern-c`, `fern-qbe`, `fern-test-supervisor`, `libfern_runtime.a`, and `fern-package.json` together under `/usr/local/bin`; install license notices under `/usr/local/share/fern`
- `PREFIX="$HOME/.local" mise run install` - Install locally without administrator privileges
- `DESTDIR=/tmp/package PREFIX=/usr/local mise run install` - Stage an installation for packaging
- `mise run uninstall` - Remove the installed component set and license notices (use the same `PREFIX`/`DESTDIR`)

### Debugging

- `mise run memcheck` - Run with Valgrind for memory leak detection

### Code Quality

- `mise run fmt` - Format C code with clang-format
- `mise run rust-fmt` - Check Rust formatting with rustfmt
- `mise run check` - Native build/test/examples/style workflow plus explicit Python integration gates
- `mise run style` - Native style checks without Python or Cargo
- `mise run style-parity` - Compare source-compiled and cached native checkers with Python
- `mise run style-launcher-check` - Native launcher/cache/process infrastructure tests

See [native checker configuration and cache cleanup](docs/NATIVE_STYLE_CHECKER.md).

## Project Structure

```
fern/
├── compiler-rs/ # Default Rust compiler, editor tooling and compiler tests
├── src/         # C reference compiler entry point and native adapters
├── runtime/     # Native C runtime
├── deps/        # Vendored QBE and other native dependencies
├── lib/         # C reference frontend and internal safety libraries
├── include/     # Header files
├── tests/       # Test suite
├── build/       # Build artifacts (generated)
├── bin/         # Compiled binaries (generated)
└── examples/    # Example Fern programs
```

## Running the Compiler

```bash
# After building
./bin/fern run examples/tiny_cli.fn
./bin/fern build examples/tiny_cli.fn -o hello
./hello
```

## Running Tests

```bash
mise run test
```

All tests should pass. If any test fails, please report it as a bug.

## Development Workflow

### First Time Setup

```bash
# Install git hooks for automatic quality checks
./scripts/install-hooks.sh
```

This installs a pre-commit hook that automatically:
- Compiles code with strict warnings
- Runs all tests
- Checks for common mistakes (malloc/free, manual unions, etc.)
- Reminds you to update ROADMAP.md

### Daily Development

1. Make changes to source code
2. Run focused tests, then `mise run test` and `mise run rust-check` to verify
3. Update ROADMAP.md to track verified progress
4. Run `mise run check`, then commit (pre-commit hook runs automatically)

**Note:** The pre-commit hook will prevent commits if tests fail or code doesn't compile.

## Compiler Profiles

The Rust compiler uses Cargo debug and release profiles. Its release profile
enables thin LTO, one codegen unit and stripped symbols. The following flags apply
to the C reference compiler and native C components.

### C Debug Build

- `-std=c11` - C11 standard
- `-Wall -Wextra -Wpedantic -Werror` - All warnings as errors
- `-g` - Debug symbols
- `-O0` - No optimization
- `-DDEBUG` - Debug mode defines

### C Release Build

- `-std=c11` - C11 standard
- `-Wall -Wextra -Wpedantic -Werror` - All warnings as errors
- `-O2` - Optimization level 2
- `-DNDEBUG` - Release mode (disables asserts)

## Troubleshooting

### "clang: command not found"

Install clang:
```bash
# macOS
xcode-select --install

# Ubuntu/Debian
sudo apt-get install clang

# Fedora
sudo dnf install clang
```

### "mise: command not found"

Install mise using [its official instructions](https://mise.jdx.dev/installing-mise.html),
then run `mise install` from this checkout. On macOS, `brew install mise` is supported.
No shell activation is required for `mise run` or `mise exec`.

### Tests fail

1. Run `mise run clean` to remove stale build artifacts
2. Run `mise run test` again
3. If still failing, check the error message and report a bug

### "ld: cannot find -lsqlite3" (or sqlite link errors)

Install SQLite development headers/libraries:
```bash
# macOS
brew install sqlite

# Ubuntu/Debian
sudo apt-get install libsqlite3-dev

# Fedora
sudo dnf install sqlite-devel
```

## Relocatable installations

`fern` locates its native components beside the actual compiler executable,
including when invoked through `PATH` or a symlink. Move `fern`, `fern-qbe`,
`fern-test-supervisor`, `libfern_runtime.a`, and `fern-package.json` together; keep
`fern-c` alongside them when retaining the reference compiler. The package marker
disables implicit development-checkout fallback. Explicit `FERN_QBE`,
`FERN_RUNTIME_LIB`, and `FERN_TEST_SUPERVISOR` overrides select individual components.
Keep the native development libraries installed for subsequent compilation.
Generated executables may depend on platform shared libraries; the release is not universally static.

`fern run` uses a private temporary directory, so simultaneous runs cannot collide
with another source file's basename. Build output paths can contain spaces,
quotes, and literal dollar signs.

If a quality check reports a nonexistent linker search directory, inspect the
shell's `LIBRARY_PATH`. Remove stale entries for that invocation (for example,
`env -u LIBRARY_PATH mise run check`); do not suppress compiler warnings globally.

## Next Steps

See [ROADMAP.md](ROADMAP.md) for active priorities and [docs/README.md](docs/README.md) for the full documentation map.
