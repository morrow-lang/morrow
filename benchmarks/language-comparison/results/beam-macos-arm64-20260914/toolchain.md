> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

# Private BEAM comparison toolchain

Prepared 2026-09-14 on macOS 26.5.1 ARM64. No Homebrew install/link command or global mise configuration was changed. `brew fetch --force-bottle erlang elixir` downloaded the bottles, then `tar -xzf` extracted them under this directory. No extracted file was patched.

## Invocation

Set `ERL_ROOTDIR=/tmp/fern-beam-tools-20260914/erlang/29.0.6/lib/erlang` and prepend `/tmp/fern-beam-tools-20260914/erlang/29.0.6/bin` to PATH.

Elixir: `/tmp/fern-beam-tools-20260914/elixir/1.20.4/bin/elixir`

Elixirc: `/tmp/fern-beam-tools-20260914/elixir/1.20.4/bin/elixirc`

Erl: `/tmp/fern-beam-tools-20260914/erlang/29.0.6/bin/erl`

Verified output: Erlang/OTP 29 [erts-17.0.6] [source] [64-bit] [smp:10:10] [ds:10:10:10] [async-threads:1] [jit] [dtrace]; Elixir 1.20.4 (compiled with Erlang/OTP 29).

`:erlang.system_info/1` reports `emu_flavor: :jit`, schedulers 10, schedulers_online 10, logical_processors_available 10, wordsize 8, otp_release '29'.

## Artifact provenance

Erlang bottle URL: https://ghcr.io/v2/homebrew/core/erlang/blobs/sha256:60e6425e089726bcae182f1856b01aa88de2782b94dedf71559e8efbc5eea0f3

Erlang bottle SHA-256 (verified locally): `60e6425e089726bcae182f1856b01aa88de2782b94dedf71559e8efbc5eea0f3`

Elixir bottle URL: https://ghcr.io/v2/homebrew/core/elixir/blobs/sha256:2db0649adb38e6bcbdb1198d43851886fb1dda4585a643df3752b9ae2c6d9206

Elixir bottle SHA-256 (verified locally): `2db0649adb38e6bcbdb1198d43851886fb1dda4585a643df3752b9ae2c6d9206`

Homebrew metadata tap Git head: `69bd7f2ba69c855b8c134a1fbdcd15da42a6d3c0`.

Extracted beam.smp SHA-256: `ebcc829632de4c8c66fb69899a87e7f3b379f12cb41b525c84df787d3937082e`

Elixir launcher SHA-256: `5cb23d89a78f75589b06fade3097e217b77e63416d61c1800f674579748c4307`

Elixirc launcher SHA-256: `448cc9ffdc2f23604eccfe370076b5afde6069a97e47dcb8d42fe22d1e169ed1`

The source releases referenced by bottle metadata are https://github.com/erlang/otp/releases/download/OTP-29.0.6/otp_src_29.0.6.tar.gz (SHA-256 `36c89ffdac9d7531c19be0cee34355b167ea95188625d32bee61ebf49ac82afa`) and https://github.com/elixir-lang/elixir/archive/refs/tags/v1.20.4.tar.gz (SHA-256 `2f87be1702583ecbeee82c0ad4d6353de96463cfa0fa6e7557e05f68d90da869`). These source archives were not downloaded or built here.

## Scope and size

Extracted install consumes 263 MiB (`du -sh`). This is a full bottle directory size, not a minimal deployed application/release size. Math/list benchmarking and compiler/version execution were verified. No benchmark timings were collected during installation.

The base beam.smp links only Apple system libraries/frameworks according to `otool -L`. Optional Homebrew ODBC/wxWidgets dependencies were not installed; native crypto/ODBC/wx applications are outside this benchmark and were not validated. This privately extracted unrelocated bottle is not asserted to be a complete general-purpose release installation.
