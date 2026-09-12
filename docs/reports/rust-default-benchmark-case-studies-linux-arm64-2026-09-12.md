# Fern Benchmark Report

## Environment
- timestamp: `2026-09-12 02:10:32 UTC`
- platform: `Linux-7.0.0-27-generic-aarch64-with-glibc2.43`
- machine: `aarch64`
- python: `3.14.7`
- clang: `Ubuntu clang version 21.1.8 (6ubuntu1)`
- git_head: `6a7bae8e2f30256b5401a13adf85ad07a1f774c3`

## Compiler Baseline
- release build: skipped (`--skip-release-build`)
- `bin/fern` size: `3806232` bytes
- `fern --version` startup (30 runs): median `0.37ms`, p95 `0.42ms`, max `0.43ms`

## Case Studies
### tiny_cli
- source: `examples/tiny_cli.fn`
- build time: `0.03s`
- executable size: `468312` bytes
- run latency (10 runs): median `0.68ms`, p95 `0.78ms`, max `0.78ms`

### http_api
- source: `examples/http_api.fn`
- build time: `0.03s`
- executable size: `468288` bytes
- run latency (10 runs): median `1.03ms`, p95 `1.52ms`, max `1.52ms`

### actor_app
- source: `examples/actor_app.fn`
- build time: `0.03s`
- executable size: `533888` bytes
- run latency (10 runs): median `0.67ms`, p95 `0.75ms`, max `0.75ms`

## Reproduce
```bash
mise run release
python3 scripts/publish_benchmarks.py --startup-runs 30 --case-runs 10
# For faster local iteration:
python3 scripts/publish_benchmarks.py --skip-release-build --startup-runs 30 --case-runs 10
```

## Related Report
- `docs/reports/memory-path-comparison-2026-02-06.md`
