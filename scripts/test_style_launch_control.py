#!/usr/bin/env python3
"""Cold-launch PID owns the build child; only a complete literal handoff can run."""
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def prepare(directory, supervisor, helper, label, behavior):
    """Make a private invocation and worker whose output is deliberately not checker output."""
    work = directory / ("work." + label)
    work.mkdir(mode=0o700)
    (work / "owner").write_text("FERN_STYLE_WORK_V1\n")
    worker = directory / (label + ".sh")
    worker.write_text('''#!/bin/bash
set -eu
umask 077
work=$1
helper=$2
supervisor=$3
behavior=$4
printf 'build stdout\\n'
printf 'build stderr\\n' >&2
if [[ $behavior == wait ]]; then
    printf 'ready\\n' > "$work/ready"
    exec /bin/sleep 3
fi
if [[ $behavior == fail ]]; then exit 7; fi
if [[ $behavior == missing ]]; then exit 0; fi
run=${work%/*}/run.${work##*.}
mkdir -m 700 "$run"
printf 'FERN_STYLE_RUN_V1\\n' > "$run/owner"
ln "$helper" "$run/program"
ln "$supervisor" "$run/supervisor"
identity() {
    if [[ $(uname -s) == Darwin ]]; then stat -f '%d:%i' "$1"; else stat -c '%d:%i' "$1"; fi
}
{ printf '%s\\n' "$run"; identity "$run"; identity "$run/program"; identity "$run/supervisor"; } > "$work/handoff"
if [[ $behavior == replaced ]]; then
    rm "$run/program"
    cp "$helper" "$run/program"
fi
''')
    return work, [str(supervisor), "--launch", str(work), str(worker), str(helper), str(supervisor), behavior, "--"]


def cases(directory, supervisor, helper):
    """Check status/streams, invalid publications, closed stdio and cold-PID-only interruption."""
    probe = directory / "umask-probe"
    probe.write_text("#!/bin/bash\numask\n")
    probe.chmod(0o700)
    for mask in (0o000, 0o002, 0o027):
        _, command = prepare(directory, supervisor, probe, "umask" + str(mask), "ok")
        result = subprocess.run(command, capture_output=True, timeout=5, umask=mask)
        assert (result.returncode, result.stdout, result.stderr) == (0, f"{mask:04o}\n".encode(), b""), result
    for status in (0, 7, 127):
        work, command = prepare(directory, supervisor, helper, "status" + str(status), "ok")
        result = subprocess.run([*command, "exit", str(status)], capture_output=True, timeout=5)
        assert (result.returncode, result.stdout, result.stderr) == (status, b"", b""), result
        assert not (directory / ("run.status" + str(status))).exists()
        assert not work.exists(), "completed invocation directory was not removed"
    _, command = prepare(directory, supervisor, helper, "streams", "ok")
    result = subprocess.run([*command, "streams"], capture_output=True, timeout=5)
    assert (result.returncode, result.stdout, result.stderr) == (7, "out\0🌿".encode(), b"err\n"), result
    for behavior in ("missing", "replaced", "fail"):
        _, command = prepare(directory, supervisor, helper, behavior, behavior)
        result = subprocess.run([*command, "exit", "0"], capture_output=True, timeout=5)
        assert result.returncode == 125 and result.stdout == b"" and b"fern style:" in result.stderr, result
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        work, command = prepare(directory, supervisor, helper, "signal" + str(sig), "wait")
        process = subprocess.Popen([*command, "exit", "0"], stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, start_new_session=True)
        for _ in range(200):
            if (work / "ready").exists():
                break
            time.sleep(0.01)
        else:
            raise AssertionError("worker not ready")
        process.send_signal(sig)  # ONLY launcher PID, never its process group.
        out, err = process.communicate(timeout=5)
        assert process.returncode == 128 + sig and out == b"" and b"interrupted" in err, (process.returncode, out, err)
    for mask in range(8):
        _, command = prepare(directory, supervisor, helper, "fds" + str(mask), "ok")
        def close_stdio():
            for fd in range(3):
                if mask & (1 << fd):
                    os.close(fd)
        result = subprocess.run([*command, "fds"], capture_output=True, timeout=5, preexec_fn=close_stdio)
        assert result.returncode == mask, (mask, result)


def main():
    """Build only standalone isolated sources in each verification mode."""
    environment = dict(os.environ, ASAN_OPTIONS="detect_leaks=0", UBSAN_OPTIONS="halt_on_error=1")
    environment.pop("LIBRARY_PATH", None)
    os.environ.update(ASAN_OPTIONS=environment["ASAN_OPTIONS"], UBSAN_OPTIONS=environment["UBSAN_OPTIONS"])
    with tempfile.TemporaryDirectory(prefix="fern-style-launch-control-") as temporary:
        directory = Path(temporary)
        helper = directory / "child"
        common = ["clang", "-std=c11", "-Wall", "-Wextra", "-Werror"]
        subprocess.run([*common, ROOT / "tests/fixtures/style_supervisor_child.c", "-o", helper], check=True, env=environment)
        helper.chmod(0o700)
        for name, flags in (("debug", ["-O0", "-g"]), ("release", ["-O2", "-DNDEBUG"]),
                            ("sanitized", ["-O1", "-g", "-fsanitize=address,undefined"])):
            supervisor = directory / name
            subprocess.run([*common, *flags, ROOT / "scripts/bootstrap/style_supervisor.c", "-o", supervisor], check=True, env=environment)
            supervisor.chmod(0o700)
            target = directory / (name + "-cases")
            target.mkdir(mode=0o700)
            cases(target, supervisor, helper)
            print(name + ": 21 launch-control cases passed")


if __name__ == "__main__":
    main()
