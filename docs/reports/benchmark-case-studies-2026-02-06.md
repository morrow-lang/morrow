> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

# Fern Benchmark Report

## Environment
- timestamp: `2026-02-06 16:00:51 UTC`
- platform: `macOS-26.2-arm64-arm-64bit-Mach-O`
- machine: `arm64`
- python: `3.14.2`
- clang: `Apple clang version 17.0.0 (clang-1700.6.3.2)`
- git_head: `d91dca4d3ddd4308ef37e87c9fa3e3f3988aeb7f`

## Compiler Baseline
- release build: skipped (`--skip-release-build`)
- `bin/fern` size: `705544` bytes
- `fern --version` startup (20 runs): median `1.56ms`, p95 `1.94ms`, max `2.06ms`

## Case Studies
### tiny_cli
- source: `examples/tiny_cli.fn`
- build time: `0.05s`
- executable size: `275128` bytes
- run latency (10 runs): median `2.57ms`, p95 `2.94ms`, max `2.94ms`

### http_api
- source: `examples/http_api.fn`
- build time: `0.06s`
- executable size: `275160` bytes
- run latency (10 runs): median `2.28ms`, p95 `3.04ms`, max `3.04ms`

### actor_app
- source: `examples/actor_app.fn`
- build time: `0.06s`
- executable size: `275096` bytes
- run latency (10 runs): median `2.33ms`, p95 `2.60ms`, max `2.60ms`

## Reproduce
```bash
just release
python3 scripts/publish_benchmarks.py --startup-runs 20 --case-runs 10
# For faster local iteration:
python3 scripts/publish_benchmarks.py --skip-release-build --startup-runs 20 --case-runs 10
```

## Related Report
- `docs/reports/memory-path-comparison-2026-02-06.md`
