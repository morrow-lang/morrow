#!/usr/bin/env python3
"""Execute the bounded process contract through C and Rust native source programs."""
import argparse
import os
from pathlib import Path
import sys
import tempfile
from test_rust_numeric import run

ROOT = Path(__file__).resolve().parents[1]
HELPER = r'''import os, signal, sys, time
mode = sys.argv[1]
if mode == 'text':
    os.write(1, 'literal ; $() " 🌿'.encode())
    os.write(2, b'errors')
    sys.exit(17)
elif mode == 'empty':
    sys.exit(127)
elif mode == 'stdin':
    os.write(1, b'eof' if not sys.stdin.buffer.read() else b'input')
elif mode == 'exact':
    os.write(1, b'x' * 4096)
    os.write(2, b'y' * 4096)
elif mode == 'out_over':
    os.write(1, b'x' * 4097)
elif mode == 'err_over':
    os.write(2, b'y' * 4097)
elif mode == 'nul':
    os.write(1, b'zero\x00byte')
elif mode == 'utf8':
    os.write(2, b'\xff')
elif mode == 'split':
    for part in [b'\xf0', b'\x9f', b'\x8c', b'\xbf']:
        os.write(1, part)
        time.sleep(.005)
elif mode == 'timeout':
    time.sleep(5)
elif mode == 'signal':
    os.kill(os.getpid(), signal.SIGTERM)
'''
PREFIX = '''fn inspect(args: List(String), timeout: Int, cap: Int) -> Unit:
    match System.exec_args_bounded(args, timeout, cap):
        Ok((status, out, err)) ->
            println(status)
            println(out)
            println(err)
        Err(code) -> println(code + 100)
'''


def execute(compiler, source, argv, expected, directory, environment):
    """Build actual source, then assert exact status and both captured streams."""
    path = directory / 'program.fn'
    path.write_text(source)
    binary = directory / 'program'
    built = run([compiler, 'build', path, '-o', binary], environment, directory)
    assert built.returncode == 0, (compiler, source, built)
    actual = run([binary, *argv], environment, directory)
    assert (actual.returncode, actual.stdout, actual.stderr) == (0, expected, ''), (compiler, source, actual)


def valid_cases(compiler, directory, environment, helper):
    """Verify real normal/error execution with independently limited text streams."""
    cases = [
        ('text', 1000, 4096, '17\nliteral ; $() " 🌿\nerrors\n'),
        ('empty', 1000, 0, '127\n\n\n'),
        ('stdin', 1000, 4096, '0\neof\n\n'),
        ('exact', 1000, 4096, '0\n' + 'x'*4096 + '\n' + 'y'*4096 + '\n'),
        ('out_over', 1000, 4096, '104\n'),
        ('err_over', 1000, 4096, '104\n'),
        ('text', 1000, 0, '104\n'),
        ('nul', 1000, 4096, '106\n'),
        ('utf8', 1000, 4096, '106\n'),
        ('split', 1000, 4096, '0\n🌿\n\n'),
        ('timeout', 50, 4096, '103\n'),
        ('signal', 1000, 4096, '107\n'),
    ]
    for mode, timeout, cap, expected in cases:
        source = PREFIX + f'fn main(): inspect([System.arg(1), System.arg(2), System.arg(3)], timeout: {timeout}, cap: {cap})\n'
        execute(compiler, source, [sys.executable, helper, mode], expected, directory, environment)
    return len(cases)


def invalid_limits(compiler, directory, environment):
    """Invalid wide values remain invalid through literals, locals and helper arithmetic."""
    expressions = ['0', '-1', '600001', '4294967297', '-4294967295',
                   '9223372036854775807', '-9223372036854775807',
                   '4294967296 + 1', 'wide()']
    for expr in expressions:
        source = PREFIX + 'fn wide() -> Int: 4294967297\nfn main():\n'
        source += f'    let timeout = {expr}\n    inspect(["missing-fern-process-executable"], timeout: timeout, cap: 0)\n'
        execute(compiler, source, [], '101\n', directory, environment)
    for expr in ['-1', '16777217', '4294967296', '-4294967296', '9223372036854775807']:
        source = PREFIX + f'fn main(): inspect(["missing-fern-process-executable"], timeout: 1000, cap: {expr})\n'
        execute(compiler, source, [], '101\n', directory, environment)
    for args in ['[]', '[""]']:
        source = PREFIX + f'fn main(): inspect({args}, timeout: 1000, cap: 0)\n'
        execute(compiler, source, [], '101\n', directory, environment)
    source = PREFIX + 'fn main(): inspect(["missing-fern-process-executable"], timeout: 1000, cap: 0)\n'
    execute(compiler, source, [], '102\n', directory, environment)
    return len(expressions) + 8



def callable_cases(compiler, directory, environment, helper):
    """Preserve Result payloads through a typed function value and propagation helper."""
    source = '''fn invoke(args: List(String)) -> Result((Int, String, String), Int):
    let call: (List(String), Int, Int) -> Result((Int, String, String), Int) = System.exec_args_bounded
    let result = call(args, 1000, 4096)?
    Ok(result)
fn main():
    match invoke([System.arg(1), System.arg(2), System.arg(3)]):
        Ok((status, out, err)) ->
            println(status)
            println(out)
            println(err)
        Err(code) -> println(code + 100)
'''
    execute(compiler, source, [sys.executable, helper, 'text'],
            '17\nliteral ; $() " 🌿\nerrors\n', directory, environment)
    execute(compiler, source, ['missing-fern-process-executable', helper, 'text'],
            '102\n', directory, environment)
    return 2



def rejected_cases(compiler, directory, environment):
    """Reject wrong signatures and discarded Results without replacing existing output."""
    sources = [
        'fn main(): System.exec_args_bounded([1], 1000, 4096)\n',
        'fn main(): System.exec_args_bounded(["tool"], "1000", 4096)\n',
        'fn main(): System.exec_args_bounded(["tool"], 1000, 1.5)\n',
        'fn bad() -> (Int, String, String): System.exec_args_bounded(["tool"], 1000, 4096)\nfn main(): ()\n',
        'fn main():\n    System.exec_args_bounded(["tool"], 1000, 4096)\n    ()\n',
    ]
    path = directory / 'rejected.fn'
    binary = directory / 'preserved-output'
    for source in sources:
        path.write_text(source)
        binary.write_bytes(b'preserved executable\x00\xff')
        binary.chmod(0o751)
        before = binary.stat()
        actual = run([compiler, 'build', path, '-o', binary], environment, directory)
        assert actual.returncode == 1 and 'error:' in actual.stderr, (compiler, source, actual)
        after = binary.stat()
        assert binary.read_bytes() == b'preserved executable\x00\xff'
        assert (after.st_mode, after.st_mtime_ns) == (before.st_mode, before.st_mtime_ns)
    return len(sources)


def main():
    """Accept isolated compilers/artifacts while preserving default project gate behavior."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, action='append')
    options = parser.parse_args()
    compilers = options.compiler or [ROOT/'bin/fern-c', ROOT/'compiler-rs/target/debug/fern-rs']
    environment = dict(os.environ)
    environment.setdefault('FERN_QBE', str(ROOT/'bin/fern-qbe'))
    environment.setdefault('FERN_RUNTIME_LIB', str(ROOT/'bin/libfern_runtime.a'))
    with tempfile.TemporaryDirectory(prefix='fern-process-source-') as temporary:
        directory = Path(temporary)
        helper = directory / 'literal helper " 🌿.py'
        helper.write_text(HELPER)
        for compiler in compilers:
            total = valid_cases(compiler.resolve(), directory, environment, helper)
            total += invalid_limits(compiler.resolve(), directory, environment)
            total += callable_cases(compiler.resolve(), directory, environment, helper)
            rejected = rejected_cases(compiler.resolve(), directory, environment)
            print(f'Bounded process source passed: {compiler.name}: {total} native cases, {rejected} atomic rejections')


if __name__ == '__main__':
    main()
