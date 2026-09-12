#!/usr/bin/env python3
"""Run full-width integer regressions through the explicit C reference compiler and native runtime."""
import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def run_case(compiler, source, directory):
    """Build one bounded fixture and require exact process status, stdout, and stderr."""
    binary = directory / source.stem
    built = subprocess.run([str(compiler), 'build', '-o', str(binary), str(source)],
                           cwd=ROOT, capture_output=True, text=True, timeout=90)
    if built.returncode != 0:
        return f'{source.name}: build failed: {built.stdout}{built.stderr}'
    try:
        result = subprocess.run([str(binary)], capture_output=True, text=True, timeout=5)
    except subprocess.TimeoutExpired:
        return f'{source.name}: execution exceeded five seconds'
    expected = source.with_suffix('.stdout').read_text()
    if (result.returncode, result.stdout, result.stderr) != (0, expected, ''):
        return f'{source.name}: expected {expected!r}; got {result.returncode}, {result.stdout!r}, {result.stderr!r}'
    return None


def main():
    """Report every independent failure so one regression cannot mask other integer boundaries."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, default=ROOT / 'bin/fern-c')
    args = parser.parse_args()
    failures = []
    cases = sorted((ROOT / 'tests/int64').glob('*.fn'))
    with tempfile.TemporaryDirectory(prefix='fern-c-int64-native-') as directory:
        for source in cases:
            failure = run_case(args.compiler.resolve(), source, Path(directory))
            if failure:
                failures.append(failure)
    for failure in failures:
        print(f'FAIL {failure}')
    print(f'C Int64 native: {len(cases) - len(failures)}/{len(cases)} passed')
    return bool(failures)


if __name__ == '__main__':
    raise SystemExit(main())
