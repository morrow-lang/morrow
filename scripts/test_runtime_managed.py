#!/usr/bin/env python3
"""Independent native105A managed actor lifecycle, ABI, and resource specifications."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def execute(argv, environment):
    """Compile or run one finite normally exiting test; failures retain exact diagnostics."""
    result = subprocess.run(list(map(str, argv)), env=environment, cwd=ROOT,
                            capture_output=True, text=True, timeout=30)
    if result.returncode:
        raise AssertionError((argv, result.returncode, result.stdout, result.stderr))
    return result.stdout


def main():
    """Use separate debug, optimized, and sanitized artifacts without altering repository builds."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cc", default=os.environ.get("CC", "cc"))
    args = parser.parse_args()
    environment = dict(os.environ)
    environment.pop("LIBRARY_PATH", None)
    variants = {
        "debug": ["-O0", "-g"],
        "release": ["-O2", "-DNDEBUG"],
        "sanitized": ["-O1", "-g", "-fsanitize=address,undefined", "-fno-omit-frame-pointer"],
    }
    with tempfile.TemporaryDirectory(prefix="fern-managed-runtime-") as temporary:
        for mode, flags in variants.items():
            for fixture in ("scheduler", "ownership", "timers"):
                binary = Path(temporary) / f"{fixture}-{mode}"
                command = [args.cc, "-std=c11", "-Wall", "-Wextra", "-Werror",
                           "-Wno-unused-function", "-Iruntime", *flags,
                           ROOT / f"tests/actors105/{fixture}.c"]
                if fixture == "scheduler":
                    command.append(ROOT / "runtime/fern_managed.c")
                if fixture == "timers":
                    command.extend(["-Iinclude", ROOT / "lib/arena.c", ROOT / "lib/fernsim.c"])
                execute([*command, "-o", binary], environment)
                output = execute([binary], environment)
                if not output.endswith(": ok\n"):
                    raise AssertionError((fixture, mode, output))
                print(f"managed actor {fixture} {mode}: passed")


if __name__ == "__main__":
    main()
