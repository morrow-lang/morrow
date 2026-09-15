> Fern was renamed to Morrow on 2026-09-15; this historical record retains its original names, paths and measurements.

# Fern Benchmark Report

## Environment
- timestamp: `2026-09-12 02:08:36 UTC`
- platform: `macOS-26.5.1-arm64-arm-64bit-Mach-O`
- machine: `arm64`
- python: `3.14.7`
- clang: `Apple clang version 21.0.0 (clang-2100.1.1.101)`
- git_head: `10c19a35bf93e69eb4d92d267a9884dfe4650c21`

## Compiler Baseline
- release build: skipped (`--skip-release-build`)
- `bin/fern` size: `3823744` bytes
- `fern --version` startup (30 runs): median `2.02ms`, p95 `2.15ms`, max `2.23ms`

## Case Studies
### tiny_cli
- source: `examples/tiny_cli.fn`
- build time: `0.06s`
- executable size: `395384` bytes
- run latency (10 runs): median `3.78ms`, p95 `5.00ms`, max `5.00ms`

### http_api
- source: `examples/http_api.fn`
- build time: `0.06s`
- executable size: `395336` bytes
- run latency (10 runs): median `5.00ms`, p95 `7.45ms`, max `7.45ms`

### actor_app
- source: `examples/actor_app.fn`
- build time: `0.07s`
- executable size: `395352` bytes
- run latency (10 runs): median `3.40ms`, p95 `4.21ms`, max `4.21ms`

## Reproduce
```bash
mise run release
python3 scripts/publish_benchmarks.py --startup-runs 30 --case-runs 10
# For faster local iteration:
python3 scripts/publish_benchmarks.py --skip-release-build --startup-runs 30 --case-runs 10
```

## Related Report
- `docs/reports/memory-path-comparison-2026-02-06.md`
