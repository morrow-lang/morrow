#!/usr/bin/env python3
"""Verify HOME cache selection and private bootstrap files under permissive caller umasks."""
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    """Exercise actual native lookup and cleanup without writing the user's cache."""
    expected = subprocess.run([ROOT / 'bin/check_style', '--help'], check=True,
                              capture_output=True, timeout=5).stdout
    with tempfile.TemporaryDirectory(prefix='fern-style-default-cache-') as temporary:
        home = Path(temporary) / 'home'
        home.mkdir(mode=0o700)
        environment = dict(os.environ, HOME=str(home))
        environment.pop('FERN_STYLE_CACHE', None)
        environment.pop('XDG_CACHE_HOME', None)
        for empty in (False, True):
            if empty:
                environment['XDG_CACHE_HOME'] = ''
            result = subprocess.run([ROOT / 'scripts/check_style', '--help'],
                                    env=environment, capture_output=True, timeout=180, umask=0o002)
            assert (result.returncode, result.stdout, result.stderr) == (0, expected, b''), result
            assert list((home / '.cache/fern-style').rglob('ready')), 'HOME cache was not populated'
        result = subprocess.run([ROOT / 'scripts/clean_style_cache'], env=environment,
                                capture_output=True, timeout=15)
        assert (result.returncode, result.stdout, result.stderr) == (0, b'', b''), result
        assert not list((home / '.cache/fern-style').rglob('ready')), 'empty XDG selected a different cache'
    print('Unset/empty XDG share the private HOME cache under caller umask002 and explicit cleanup')


if __name__ == '__main__':
    main()
