# Morrow benchmarks

Run these commands from the repository root with the pinned Rust toolchain.
The Morrow programming language uses `.mr` sources and the `morrow` command.

> Fern was renamed to Morrow on 2026-09-15. The linked historical reports retain their original names, commands, source paths, hashes and measurements; the commands here use the current project names.

## Compiler phases

```sh
cargo test --manifest-path benchmarks/compiler-phases/Cargo.toml --locked
cargo bench --manifest-path benchmarks/compiler-phases/Cargo.toml --locked --bench phases -- --test
```

See the [phase methodology and historical measurements](compiler-phases/README.md).

## Language comparison

Build the optimized compiler/runtime and the independent Rust harness. Set `BUN`
and `TSC` to the executables described in the [original report](language-comparison/README.md).
Choose a fresh results directory; measurement uses macOS `/usr/bin/time` conventions.

```sh
cargo build -p morrow -p morrow-runtime-native --release
rustc --edition=2024 -Dwarnings benchmarks/language-comparison/src/main.rs -o /tmp/morrow-language-runner
rustc --edition=2024 --test benchmarks/language-comparison/src/oracle.rs -o /tmp/morrow-language-oracles
/tmp/morrow-language-oracles
export MORROW="$PWD/target/release/morrow"
export MORROW_RUNTIME_LIB="$PWD/target/release/libmorrow_runtime_native.a"
export BUN="/path/to/bun"
export TSC="/path/to/typescript/bin/tsc"
/tmp/morrow-language-runner prepare /tmp/morrow-language-results-new
/tmp/morrow-language-runner verify /tmp/morrow-language-results-new
/tmp/morrow-language-runner measure /tmp/morrow-language-results-new
/tmp/morrow-language-runner measure-check-policy /tmp/morrow-language-results-new
/tmp/morrow-language-runner summary /tmp/morrow-language-results-new
```

Historical follow-ups cover [immutable callbacks](language-comparison/IMMUTABLE.md),
[native optimization](language-comparison/NATIVE_OPTIMIZATION.md),
[integer arithmetic](language-comparison/ARITHMETIC.md) and
[Elixir/BEAM](language-comparison/BEAM.md). New runs need their own source identity
and environment record; the rename does not establish new performance results.

## Network codecs and compiled application

```sh
cargo test --manifest-path benchmarks/network-codecs/Cargo.toml --locked
cargo run --release --manifest-path benchmarks/network-codecs/Cargo.toml --locked -- 100 9 > morrow-codec-results.json
cargo test --manifest-path benchmarks/message-path/Cargo.toml --locked
cargo run --release --manifest-path benchmarks/message-path/Cargo.toml --locked -- 150 20 5 > morrow-message-path-results.json
```

See the original [codec measurements](network-codecs/README.md),
[browser/WASM experiment](network-codecs/BROWSER.md) and
[compiled application round trips](message-path/README.md). Their JSON results and
source hash metadata remain frozen. Current benchmark sources and independent
lockfiles follow the renamed workspace.
