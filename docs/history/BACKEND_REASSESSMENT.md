> Historical document, retained for architectural context. Its implementation paths, commands and measurements describe the retired setup. See [the current build guide](../../BUILD.md) and [roadmap](../../ROADMAP.md).

# Fern backend reassessment — 6 September 2026

Recommendation: pursue a current, supported Cranelift AOT backend as a measured second backend, then decide whether to make it the default after Fern's full native acceptance corpus passes. Keep QBE as the working reference during that migration. Rust is a good implementation language for either choice; it does not require a Rust code generator. Cranelift is probably the alternative the user remembers.

Current status: Fern now has shared typed machine lowering and an integrated, optional Cranelift AOT backend (Decision112). QBE remains the default and reference backend. The complete feature gate passes on macOS ARM64 and Linux ARM64: 293 independent native output oracles on each platform, feature-enabled formatting/Clippy/Rust tests, and targeted mixed-argument ABI, relocation, loop and forced-GC retention tests. The Cranelift-built Fern style checker also passes strict checks on macOS. The default Rust/QBE regression gate passes on both ARM64 platforms, including native libraries, actors, JSON, packaging, workflow oracles and 192 source mutations. These local gate runs disabled Cargo incremental artifacts and compiler/test debug information to fit available disk space; generated Fern code still uses the documented Cranelift settings. Source-level debugger acceptance, controlled end-to-end performance measurements and default promotion remain open. Bounded native actors (105A) are implemented separately, while generalized suspension and supervision remain open.

The experiment below was run on 2026-09-06 before that toolchain change. Its dependencies, tools and measurements remain historical evidence. The original local prototype files and raw measurements are under `/tmp/fern-backend-research`; those temporary paths are not installed project components.

## Run the integrated trial

```sh
mise run rust-cranelift-build
./bin/fern-rs-cranelift run --backend=cranelift examples/tiny_cli.fn
./bin/fern-rs-cranelift build --backend=cranelift examples/tiny_cli.fn -o hello-cranelift
mise run rust-cranelift-check
```

The build task uses the pinned mise environment and `cargo --locked --features
cranelift`, prepares the native runtime/reference helpers, and copies the compiler
to `bin/fern-rs-cranelift`. The ordinary `bin/fern-rs` stays in place. QBE remains
the default in either executable; select Cranelift explicitly for `build` or
`run`. `emit`, native source tests and preview packaging retain their existing
QBE workflows. The check task runs feature-enabled Clippy/Rust tests and the
independent backend corpus in `scripts/test_cranelift_backend.py`.

A selected Cranelift invocation produces a native object without executing QBE
or an assembler. It still links Fern's C runtime and native libraries with the
host linker. The C reference frontend, runtime, supervisor and generated editor
parser are not removed by this integration; Python remains in developer oracles.
The change moves native code generation into Rust, not every repository component.

## What remains outside Rust

The September 2026 source inventory separates authored implementation from
third-party and generated code:

| Component | Current role | Migration boundary |
| --- | --- | --- |
| Legacy C compiler, about 23,000 lines | Explicit `fern-c` reference and bootstrap | Rust/QBE now ships as `fern` (Decision115); retaining C is independent of backend promotion |
| Authored C runtime, about 11,000 lines | Heap values, JSON, actors, IO and platform services | A separate runtime port preserving layouts, GC roots and resource contracts |
| Vendored QBE | Reference code generator | Can retire after Cranelift default acceptance |
| Generated editor parser and headers, about 90,000 lines | Tree-sitter editor integration | Generated C is the editor ecosystem's output format |
| Boehm GC, SQLite, OpenSSL and other native libraries | Runtime dependencies | Remain native dependencies even if callers are rewritten in Rust |
| Native supervisors and bootstrap helpers | Child ownership, cleanup and cached tools | Separate platform implementation and safety audit |
| Python tooling | Independent expected-output oracles, generators and developer checks | No Python interpreter is required to execute compiled Fern programs |

The intended Rust compiler migration and a runtime with no C dependencies are
different deliverables. Rewriting independent test oracles solely to change a
repository language percentage would not establish either one.

## What changed in the original rationale

Local Decision1 says QBE emits C and therefore reaches any platform with a C compiler. That is incorrect: QBE emits assembly for supported machine targets; an assembler and linker produce the executable. Its small C implementation and compact SSA interface remain real advantages. Decision23's embedded backend rationale and Decision45's Rust frontend/process boundary are independent choices. A Rust frontend can keep invoking QBE, embed it through a separately audited boundary, or emit Cranelift IR. [QBE overview](https://c9x.me/compile/), [QBE IL documentation](https://c9x.me/compile/doc/il.html).

Upstream QBE is actively maintained: release1.3 is dated 1 June2026 and adds Windows ABI support. Its archive SHA256 is `d587905d620dc5e1d2bfa7c2cc642b9b837aa89a3188c6e37b53d756cf66e320`, upstream commit `c0818978acec60ebb6167fade60fb7012cbf20ca`. Fern's January2026 vendor import should not be described as current1.3 without reconciliation. Updating QBE is a separate candidate worth benchmarking; this experiment used Fern's existing vendor plus the isolated Decision104 Apple fix. [QBE releases](https://c9x.me/compile/releases.html).

Cranelift is a general-purpose native code generator with AOT and JIT APIs, production use in Wasmtime, and x86-64, AArch64, s390x and RISC-V backends. Its advertised compilation-speed comparisons concern LLVM, not QBE or Fern. No evidence here establishes that Cranelift intrinsically compiles Fern faster or generates faster Fern programs. [Cranelift project](https://cranelift.dev/).

## Toolchain and maintenance decision

At the 2026-09-06 assessment, the verified current Cranelift release was0.135.1, in Wasmtime48.0.1. Its workspace requires Rust1.95.0 and edition2024; crates inherit that requirement. Stable Rust was1.98.1, released 3 September2026. Modern Cranelift required an upgrade from Fern's then-1.75 baseline, but did not require nightly. The modern prototype below used an already-installed Rust1.97.1, which satisfies the requirement; that experiment did not install or change the default toolchain. [Release manifest](https://raw.githubusercontent.com/bytecodealliance/wasmtime/v48.0.1/Cargo.toml), [Codegen manifest](https://raw.githubusercontent.com/bytecodealliance/wasmtime/v48.0.1/cranelift/codegen/Cargo.toml), [Rust release notes](https://doc.rust-lang.org/releases.html).

Historical Cranelift0.108.2/Wasmtime21.0.2 has a1.75 workspace minimum;0.109.1/Wasmtime22.0.1 moves to1.76. I compiled0.108.2 on1.75 only after pinning an old transitive indexmap version: unconstrained contemporary dependency resolution selected an edition2024 dependency that old Cargo cannot parse. This is concrete evidence that an old version number alone is not a maintained toolchain policy. Do not adopt that unsupported2024 release merely to preserve1.75. [Wasmtime21 manifest](https://raw.githubusercontent.com/bytecodealliance/wasmtime/v21.0.2/Cargo.toml), [Wasmtime22 manifest](https://raw.githubusercontent.com/bytecodealliance/wasmtime/v22.0.1/Cargo.toml).

Decision111 adopts the user's requested dated nightly. Decision112 pins the optional Cranelift crates to0.135.1 and records transitive inputs in Cargo.lock; the default Cargo feature set excludes Cranelift and includes the pinned Rustyline terminal dependency adopted in Decision115. Backend maintenance follows the upstream supported release window: review upstream releases/security notices, update the Cranelift crate family together, commit the refreshed lockfile, and rerun both native backend gates before adopting an update. Unsupported historical pins are comparison evidence only, never a compatibility strategy. Wasmtime publishes monthly releases and a defined support/LTS policy; Cranelift's API is not assumed permanently stable. [Support policy](https://docs.wasmtime.dev/stability-release.html).

## Concrete integration issues

At the pre-actor assessment checkpoint, Fern had approximately6315 lines of Rust QBE emitter/helpers, not just instruction printing. They implement full-width heap payloads, nominal/newtype/union layouts, closures, manual self-tail-call elimination, dynamic defers, fault propagation, checked indexing, JSON codec descriptors, public typed-IR validation and source-owned test exit behavior. Most of that must remain backend-independent and must not be reimplemented inconsistently in two emitters.

The implemented shared representation records scalar widths, typed operations/call arguments, hidden environment/fault parameters, block terminators, immutable data, relocations and scratch allocations after semantic validation. Both backends consume it directly. Existing compiler-owned helpers are structured Rust builders rather than emitted-text input to another parser. Source locations and debugger variable mappings remain future work; GC-visible pointer lifetimes and complete runtime ABI behavior require the acceptance gates below. Keep independent expected-output tests for all existing fault/defer/tail semantics.

Cranelift's object backend writes ELF, COFF and Mach-O directly, removing Fern's separate assembly subprocess. Linking the existing C runtime and its libraries remains necessary. A compiler built with Cranelift does not automatically link Rust std or Cranelift into every Fern application. Cross compilation still needs the correct runtime libraries, SDK and linker. The object backend explicitly rejects Wasm output: Wasmtime compiling Wasm into native code is a different direction from Fern targeting browser Wasm. A future browser backend still needs its own Wasm/runtime design. [Object backend implementation](https://raw.githubusercontent.com/bytecodealliance/wasmtime/v48.0.1/cranelift/object/src/backend.rs).

The integrated trial replaces generated Float `printf`/`snprintf` calls with fixed-signature C runtime wrappers for printing and string conversion. Cranelift has an open general variadic-support issue; keeping those calls inside C lets the host compiler implement its variadic ABI. Verify the wrappers and resulting native Float behavior on every supported platform. A fixed signature alone must never be assumed to reproduce Apple variadic conventions. [Upstream varargs issue](https://github.com/bytecodealliance/wasmtime/issues/1030).

Cranelift has an explicit AppleAarch64 calling convention and its AArch64 allocatable register environment excludes reserved x18, with x16/x17 scratch registers. That is encouraging evidence, not a substitute for Fern's new no-x18 and scalar-clobber regression. The Decision104 defect in Fern's QBE vendor is concrete; it remains unproven that it caused the original sampled checker crash. [Calling conventions](https://raw.githubusercontent.com/bytecodealliance/wasmtime/v48.0.1/cranelift/codegen/src/isa/call_conv.rs), [AArch64 implementation](https://raw.githubusercontent.com/bytecodealliance/wasmtime/v48.0.1/cranelift/codegen/src/isa/aarch64/abi.rs), [Apple ABI](https://developer.apple.com/documentation/xcode/writing-arm64-code-for-apple-platforms).

Debugging requires frontend work with either backend. Cranelift source locations are opaque frontend-defined IDs; Fern must provide file/line mapping and write suitable debug data/variable locations. In the current object backend, enabling its convenience `unwind_info(true)` path for Mach-O explicitly panics because that path does not implement Mach-O eh_frame. This is a specific missing object-emission path, not a claim that all Cranelift debugging on Apple is impossible. QBE already exposes experimental dbgfile/dbgloc operations, which the current Rust emitter does not use. Useful Fern line breakpoints could therefore be improved before a backend migration. [Source locations](https://raw.githubusercontent.com/bytecodealliance/wasmtime/v48.0.1/cranelift/codegen/src/ir/sourceloc.rs), [Object unwind implementation](https://raw.githubusercontent.com/bytecodealliance/wasmtime/v48.0.1/cranelift/object/src/backend.rs), [QBE releases](https://c9x.me/compile/releases.html).

A Cranelift JIT is a later project. It does not remove the REPL's effect, retained-value, GC, failure and safe-Rust boundaries. Likewise, using rustc_codegen_cranelift to compile the Fern compiler itself is a separate question from using Cranelift to compile Fern programs.

## Executed AOT experiment

Host: Apple M2 Pro, native macOS ARM64. Both backends compiled the same scalar `bench(n:i64, seed:i64)->i64` iterative xorshift algorithm; variants generated either1 or200 exported functions. An independent C reference checked inputs0..99 and a ten-million-step result, including a high-bit seed. Every native executable returned success and the final decimal result `2783892466536392950`.

Cranelift emitted Mach-O objects directly. QBE emitted assembly, then the same host C driver assembled it. Backend measurements used two warmups and ten measured processes; native runtime used six executions with the clock inside the binary, excluding startup. These are medians in milliseconds:

| Backend | Functions | Backend process | Through object creation | Native10m steps |
|---|---:|---:|---:|---:|
| Fern QBE +104 |1|2.904|41.052|18.172|
| Cranelift0.135.1 none |1|2.583|2.583|18.141|
| Cranelift0.135.1 speed |1|2.661|2.661|18.166|
| Fern QBE +104 |200|9.857|69.867|18.011|
| Cranelift0.135.1 none |200|9.711|9.711|18.601|
| Cranelift0.135.1 speed |200|12.042|12.042|18.169|

The measurable advantage here is principally avoiding the separate Apple assembler/driver stage. For200 functions, backend computation is comparable, and speed-mode Cranelift is slower to compile than QBE in this sample. Native execution is similar. The common link stage still took approximately62–66ms in individual, non-statistical observations. This is not an end-to-end Fern benchmark: QBE's input SSA was pre-serialized, Cranelift's process included IR construction, the host was not reserved, run order was not randomized, and the program has no GC, calls, Float, runtime IO or debug metadata. It supplies feasibility and a concrete pipeline hypothesis, not a production performance promise.

The1-function executables were33528 bytes with QBE and33536 with Cranelift. A Rust compiler backend dependency does not imply large generated programs. Backend utility binaries were roughly218KiB for QBE versus2.6MiB for the modern Cranelift prototype, but stripping/features/build flags differ, so this is only a rough compiler-distribution observation. Historical0.108.2 results are retained in raw JSON, not used to recommend an unsupported release.

Local experimental artifacts (not installed project components):

- `/tmp/fern-backend-research/prototype`: Rust1.75 + pinned Cranelift0.108.2, isolated Cargo home; initial MSRV failure and corrected build logs retained.
- `/tmp/fern-backend-research/current-prototype`: Cranelift0.135.1, Rust1.97.1; no shared dependency changes.
- `/tmp/fern-backend-research/measure.py`, `driver.c`, `measurements.json`, `measurements.log`.
- Measurement-script SHA256: `53f03ab31be52ed4df359dac2ee664c3fab4a4557ea8f1eef747e4c51423cac0`.
- Results SHA256: `37f6d03a71c503235df517cefaab4100d4a41ec064a1e57b82550b99d071ae99`.
- Modern lockfile SHA256: `705c8a49e661e576cc44becdc1d19203ad91421457f5bc91d14057d448691731`.
- Historical lockfile SHA256: `f502e0dcfca028293efb50d9556017a472d543d61184675828099367108dea09`.

## Acceptance and benchmark plan before changing the default

1. Preserve the Decision112 optional dependency pins, single shared semantic lowering and explicit experimental backend selection. Keep the QBE path and fixed-signature Float adapters while verifying the integrated trial. The implementation alone does not complete the following acceptance gates.
2. Run the full native Fern oracle corpus independently through both backends on AppleARM64 and LinuxARM64, then Linuxx86-64. Include full-width Int extrema, Float negative zero/NaN, Bool, more than eight arguments, mixed C ABI arguments, closures/environment/fault pointers, tuples, JSON values/codecs, nominal and unboxed newtypes, unions and relocation/PIC cases. Retain expected outputs; differential agreement alone is insufficient.
3. Pin all cleanup/failure contracts: first fault, argument evaluation order, defer LIFO including failing cleanup, HOF callbacks, Try/Return, Result main, source test exit interception, checked collection bounds, and million-iteration constant-stack recursion. Stress Boehm-visible pointers in captures/records/collections across allocations and C calls. Every malformed public IR/resource-bound regression must remain rejected before backend execution.
4. Require real debugger acceptance: source breakpoint, correct line after branch/closure, stack through Fern and C calls, and documented variable visibility on LLDB/GDB. If full debug support is deferred, state its exact level instead of describing the switch as solving debugging.
5. Measure release compiler end-to-end check/build/run time, lowering time, code generation, object creation, linking, peak RSS, compiler build time and install size separately. Use tiny CLI, actual repository checker, JSON/collection work, closures/tail recursion and a large multi-module application. Use at least30 alternating randomized runs per configuration on otherwise idle hosts, report median/p95, state cold/warm cache definitions, and preserve raw inputs, tool versions and logs. Compare current QBE, separately updated QBE if warranted, and supported Cranelift none/speed with identical runtime/link options.
6. Make the default decision from user-visible build/debug benefits, maintenance cost and native regression evidence. Retain QBE until the complete semantic gates pass and no material unexplained performance or deployment regression remains. AOT Cranelift is a credible next architecture, but the tiny prototype does not justify deleting a working backend immediately.

The measured raw result table is retained in [the report data](../reports/backend-reassessment-2026-09-06.json). Re-run the full acceptance plan above before making a backend or performance default decision.
