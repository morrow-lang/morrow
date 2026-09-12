#!/usr/bin/env python3
"""Verify explicit stderr output and preserved primary failures through both compilers."""
import argparse
import os
from pathlib import Path
import sys
import tempfile
from test_rust_numeric import run

ROOT = Path(__file__).resolve().parents[1]
DIRECT = '''fn main() -> Int:
    match System.write_stderr("diagnostic 🌿"):
        Ok(()) -> 7
        Err(code) ->
            println(code)
            7
'''
CALLABLE = '''fn output(text: String) -> Result(Unit, Int):
    let write: (String) -> Result(Unit, Int) = System.write_stderr
    let done = write(text)?
    Ok(done)
fn main() -> Int:
    match output("diagnostic 🌿"):
        Ok(()) -> 7
        Err(code) ->
            println(code)
            7
'''


def verify(compiler, directory, environment):
    """Test native streams, Unit/Result ABI and closed-descriptor failure without a crash."""
    source = directory / 'stderr.fn'
    binary = directory / 'stderr'
    count = 0
    for text in [DIRECT, CALLABLE, DIRECT.replace('diagnostic 🌿', '')]:
        source.write_text(text)
        built = run([compiler, 'build', source, '-o', binary], environment, directory)
        assert built.returncode == 0, (compiler, built)
        normal = run([binary], environment, directory)
        expected = 'diagnostic 🌿' if 'diagnostic' in text else ''
        assert (normal.returncode, normal.stdout, normal.stderr) == (7, '', expected), normal
        close = 'import os,sys; os.close(2); os.execv(sys.argv[1], [sys.argv[1]])'
        closed = run([sys.executable, '-c', close, binary], environment, directory)
        assert (closed.returncode, closed.stdout, closed.stderr) == (7, '3\n' if expected else '', ''), closed
        count += 2
    for text in ['fn main(): System.write_stderr(1)\n',
                 'fn main():\n    System.write_stderr("lost")\n    ()\n']:
        source.write_text(text)
        before = binary.read_bytes()
        rejected = run([compiler, 'build', source, '-o', binary], environment, directory)
        assert rejected.returncode == 1 and 'error:' in rejected.stderr, rejected
        assert binary.read_bytes() == before
    print(f'Stderr source passed: {compiler.name}: {count} native cases, 2 atomic rejections')


def main():
    """Accept isolated frontend paths or verify both default build artifacts."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', action='append', type=Path)
    args = parser.parse_args()
    environment = dict(os.environ)
    environment.setdefault('FERN_QBE', str(ROOT / 'bin/fern-qbe'))
    environment.setdefault('FERN_RUNTIME_LIB', str(ROOT / 'bin/libfern_runtime.a'))
    compilers = args.compiler or [ROOT / 'bin/fern-c', ROOT / 'compiler-rs/target/debug/fern-rs']
    with tempfile.TemporaryDirectory(prefix='fern-stderr-source-') as temporary:
        for compiler in compilers:
            verify(compiler.resolve(), Path(temporary), environment)


if __name__ == '__main__':
    main()
