# Fern, Rust and TypeScript/Bun experiments

These are reproducible experiments on current implementations, not a language
ranking or a developer productivity study. The programs have independent exact
output oracles; compiler rejection is measured separately from runtime behavior.
The comparison sources in Rust and TypeScript are intentional test subjects.
Repository orchestration remains Rust.

## Recorded results, 2026-09-14

Apple M4, 24 GiB RAM, macOS 26.5.1, ARM64; Fern source `472d30c`, Rust
`1.100.0-nightly (f248f4038 2026-09-05)`, Bun **1.4.2**, TypeScript **6.0.2**.
[Raw measurements, metadata, sizes and compiler diagnostics](results/macos-arm64-20260914/)
are checked in. All measured workload outputs matched the independent oracles.

| Whole-process workload | Fern median | Rust median | Bun median |
| --- | ---: | ---: | ---: |
| 20 million scalar steps | 76.42 ms | 57.25 ms | 519.91 ms Number; 246.81 ms BigInt |
| 10,000 immutable 256-entry updates | 70.94 ms | 3.59 ms | 13.88 ms |
| 10,000 updates reusing the collection | Not this language contract | 2.85 ms | 6.89 ms |
| Minimal process printing `0` | 2.54 ms | 2.47 ms | 6.01 ms |
| Check the workload source | 4.71 ms | 29.38 ms | 121.11 ms with tsc and recommended `skipLibCheck` |
| Build the workload executable | 37.82 ms | 134.87 ms | Source execution; no executable build measured |

| Peak process RSS, median | Fern | Rust | Bun |
| --- | ---: | ---: | ---: |
| Scalar | 1.50 MiB | 1.52 MiB | 18.17 MiB Number; 27.45 MiB BigInt |
| Immutable model | 3.48 MiB | 1.55 MiB | 30.33 MiB |
| Minimal startup | 1.50 MiB | 1.48 MiB | 9.22 MiB |

The tiny Fern binary is **523,656 bytes**; the combined workload binary is
**568,104 bytes**. Rust's corresponding executables are **342,464** and
**359,760 bytes**. Bun's workload source is 1,862 bytes but requires its
**61,884,464-byte runtime**; that runtime is shared by all Bun programs on an
installation. A Bun standalone executable was not measured. These are macOS
executables using system libraries, not static Linux deployment sizes.

The observations are mixed. Current Fern is within about 1.34× Rust on this
scalar recurrence and uses little process memory. Its immutable model loop is
about 19.8× slower than Rust and 5.1× slower than Bun. Native generated-code
optimization, collection representation, closure calls, allocation and collection
are candidates for profiling; this experiment does not attribute their shares.
Fast checking/building is useful for an edit/run loop, but these tiny programs do
not predict whole-project compile times. The observed BigInt advantage over
Number is specific to this recurrence and Bun version; it is a reason to measure
both, not a general recommendation to convert all JavaScript numbers to BigInt.

The TypeScript check includes `--strict --noUncheckedIndexedAccess` and Bun's
recommended `--skipLibCheck`. A separate baseline also checks standard-library
declarations and takes **359.57 ms**. Both sets of raw samples are retained;
skipping declaration checking does not skip checking the application source.

Startup samples retain large first-launch outliers: Fern **403.45 ms**, Rust
**251.56 ms**, Bun **17.39 ms**. The reported medians describe repeated launches
on this machine. The harness includes macOS loader/cache/security behavior and
an external timing wrapper; it does not establish sub-millisecond initialization
or cleanly isolate the cause of first-launch overhead. No samples were dropped.

## Workloads and fairness

* **Scalar recurrence:** 20 million steps of `x = x * 48271 mod 2147483647`,
  starting with runtime seed 7. Every intermediate is below 2^53, so both signed
  i64 and JavaScript Number calculate exact integer results in this workload.
  Bun BigInt is a separately labeled full-width-capable variant. The independent
  oracle computes modular exponentiation in logarithmic time rather than
  translating the measured loop. Fixed Park–Miller reference values and zero,
  one, two, boundary-cycle and alternate-seed cases anchor it.
* **Immutable model:** 10,000 updates of a 256-entry model, changing the entry at
  `(seed + step * 17) mod 256`. Each update maps the whole collection, preserving
  old values; the program prints a weighted final checksum and verifies that an
  independently retained original model remains zero. The oracle counts visits
  to each index using the modular inverse of 17, not the update algorithm.
  Fern shares unchanged records through its managed representation; Rust copies
  small `Cell` values into a new Vec; Bun shares unchanged readonly objects in a
  new array. These implement the same observable immutable-update semantics,
  with the ordinary representation of each language, not identical allocation.
* **In-place alternatives:** Rust and Bun also update a reusable collection.
  They preserve the initial model, but not every intermediate collection.
  This changes the implementation contract and is reported separately. It shows
  the opportunity available when the application does not need old versions.
* **Precision:** exact results above 2^53 use Fern Int, Rust i64 and Bun BigInt.
  A separate JavaScript Number fixture deliberately loses one unit. This is a
  correctness demonstration, not a claim that all JavaScript arithmetic fails.

Inputs are supplied at process startup. The runner controls their small decimal
syntax and bounds; the argument parsers are not production input validators.
Each timed sample starts a fresh process, so Bun compilation/JIT warmup within
that process is included. There is one untimed full workload warmup before nine
interleaved timed rounds. Startup has 21 samples; hot-filesystem source checking
and executable building have five. No dependency installation, compiler build,
package resolution or cold disk cache is charged to source build timings.

Fern is the existing release-built compiler linked with a release-built Rust
runtime, but **its generated native code currently uses Cranelift
`opt_level=none`**. Building the compiler in release does not enable native
optimization. Rust uses `rustc -O -C strip=symbols -C panic=abort`; Bun uses its
default JIT. This records what users can run today, not an equal-optimization
backend contest. The native binaries include runtime/library code selected by
their linkers and use the normal macOS system libraries.

Timing uses `Instant` around `/usr/bin/time -l PROGRAM`, including the time
wrapper's launch and process reaping; RSS is the macOS maximum resident set size
in bytes reported for the measured process. Sub-millisecond differences should
not be inferred from this whole-process harness. Results concern one laptop,
with other applications present; compiler/benchmark work was coordinated to
avoid overlapping measured runs. They do not measure HTTP throughput, browser
DOM work, distributed actors or sustained production tail latency.

## Reproduce

Run from the repository root with the pinned Rust toolchain. Build the release
compiler/runtime first. No Cargo package or third-party runner dependency is
needed. Set `TSC` to the `bin/tsc` file of a pinned TypeScript installation; the
recorded run uses TypeScript 6.0.2. The program invokes it through Bun.

The primary run uses Bun **1.4.2**, downloaded from the
[official release](https://github.com/oven-sh/bun/releases/tag/bun-v1.4.2), rather
than the older globally installed Bun. The `bun-darwin-aarch64.zip` asset's
GitHub-reported SHA-256 and the downloaded file both equal
`90987a3a16d7db556d886ac3d551e7b6d3edf0a1cf43acaed622e8676be1d12f`.
No global installation was changed.

```sh
cargo build -p fern -p fern-runtime-native --release
rustc --edition=2024 -Dwarnings benchmarks/language-comparison/src/main.rs -o /tmp/fern-language-runner
rustc --edition=2024 --test benchmarks/language-comparison/src/oracle.rs -o /tmp/fern-language-oracles
/tmp/fern-language-oracles
export FERN="$PWD/target/release/fern"
export FERN_RUNTIME_LIB="$PWD/target/release/libfern_runtime_native.a"
export BUN="/path/to/bun-1.4.2"
export TSC="/path/to/typescript/bin/tsc"
/tmp/fern-language-runner prepare /tmp/fern-language-results-new
/tmp/fern-language-runner verify /tmp/fern-language-results-new
/tmp/fern-language-runner measure /tmp/fern-language-results-new
/tmp/fern-language-runner measure-check-policy /tmp/fern-language-results-new
/tmp/fern-language-runner summary /tmp/fern-language-results-new
```

`prepare` requires a new output directory and writes executable artifacts there.
The harness executes fixed, bounded, trusted fixtures, not arbitrary submitted
programs. Measurement is macOS-specific because `/usr/bin/time` options and RSS
units differ across systems. The independent oracles and compilation/execution
verification can also be run on other supported native hosts.

## What the developer experiments can establish

Each of the five small mutation classes has a saved compiler diagnostic and an
explicit expected result. Rust is tested both normally and with
`-Dunused_must_use`; TypeScript is checked with `--strict`, then with
`--noUncheckedIndexedAccess` added. Bun execution itself is not type checking.
The runner also executes the intentionally non-exhaustive and unsafe-index
TypeScript fixtures under Bun: they print `pending` and `undefined`, respectively,
despite the separate type-checker errors. Positive controls make sure each
rejection reflects the intended mistake rather than invalid fixture syntax.

| Change or mistake | Fern | Rust | TypeScript checked separately from Bun execution |
| --- | --- | --- | --- |
| Add a sum variant without updating a match | Compiler rejects | Compiler rejects | Discriminated union plus `never` exhaustiveness check rejects |
| Ignore a fallible Result | Compiler rejects | Warning by default; rejection with `-Dunused_must_use` | A discarded ordinary result union is accepted; separate lint/API policy is needed |
| Omit labels for two same-type function parameters | Compiler rejects | Positional call is accepted | Positional call is accepted; an object parameter can provide named fields |
| Use ProductId where UserId is required | Nominal newtype rejects | Newtype rejects | Distinct branded types reject |
| Index past a collection boundary | Current `List.get` compiles, then faults | Indexing compiles, then panics | `--strict` alone accepts; `noUncheckedIndexedAccess` rejects assigning a possibly missing element to Number |

Labels make intent visible; they do not prove that a caller chose the right
value. Newtypes require the developer to model domain identities in every
language. Rust provides `slice.get` returning Option; Fern's currently faulting
`List.get` is a real mismatch with the broad design aspiration that all failures
are typed. Immutability constrains accidental updates but does not prove business
logic or distributed delivery correct. TypeScript readonly is a compile-time
constraint; it does not freeze runtime objects or validate external data.

These observations support a narrower claim about Fern: it puts several useful
policies into one default language experience, without application lifetime
annotations. They do not prove that a developer finishes sooner, creates fewer
bugs, or cannot express an equally correct Rust/TypeScript application. Such
claims require user studies and larger task sets. Rust offers explicit control
over layout and in-place updates; TypeScript/Bun offers direct access to an
established browser/runtime ecosystem. Fern's current implementation still has
performance and fallible-API work to do.

The model source also exposes a concrete tradeoff in expression. Fern's update
is a single immutable `List.map` with a record update and no ownership syntax.
Rust expresses the same policy with an iterator and struct update, while making
the allocation and copied representation visible. TypeScript's `map` and object
spread are similarly direct; readonly types express the intended contract.
Fern's stateful outer iteration uses a tail-recursive function with labeled
arguments. Rust/Bun use an ordinary loop. These examples are small enough to
read together under [`programs/`](programs/); none establishes that one syntax
is universally easier. Rust's in-place alternative is especially appropriate
when intermediate model versions are not needed.

Primary references, checked 2026-09-14:

* [Rust must_use and lint levels](https://doc.rust-lang.org/reference/attributes/diagnostics.html#the-must_use-attribute).
* [TypeScript exhaustiveness with never](https://www.typescriptlang.org/docs/handbook/2/narrowing.html#exhaustiveness-checking).
* [TypeScript noUncheckedIndexedAccess](https://www.typescriptlang.org/tsconfig/noUncheckedIndexedAccess.html).
* [Bun's recommended TypeScript configuration](https://bun.com/docs/typescript)
  already enables `noUncheckedIndexedAccess`; the stronger case is recommended
  setup, not an obscure capability.
* [Bun's TypeScript loader](https://bun.com/docs/runtime/file-types#ts) erases
  TypeScript syntax and does not check types.
