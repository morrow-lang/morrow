#!/usr/bin/env python3
"""Exercise real terminal editing, completion, cancellation and retained history."""
import argparse
import os
from pathlib import Path
import pty
import select
import signal
import tempfile
import time
import unittest


class Terminal:
    """A bounded PTY child with isolated on-disk REPL history."""
    def __init__(self, compiler, history):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            environment = dict(os.environ, TERM="xterm", FERN_REPL_HISTORY=str(history))
            os.execve(str(compiler), [str(compiler), "repl"], environment)
        self.pending = b""
        try:
            self.until(b"fern> ")
        except BaseException:
            os.kill(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
            os.close(self.fd)
            raise

    def until(self, expected):
        deadline = time.monotonic() + 5
        while expected not in self.pending and time.monotonic() < deadline:
            ready, _, _ = select.select([self.fd], [], [], 0.1)
            if ready:
                self.pending += os.read(self.fd, 65536)
                if b"\x1b[6n" in self.pending:
                    self.pending = self.pending.replace(b"\x1b[6n", b"")
                    os.write(self.fd, b"\x1b[1;1R")
        if expected not in self.pending:
            raise AssertionError(f"missing {expected!r}: {self.pending!r}")
        end = self.pending.index(expected) + len(expected)
        result, self.pending = self.pending[:end], self.pending[end:]
        return result

    def send(self, text):
        os.write(self.fd, text)

    def close(self):
        self.send(b":quit\n")
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                os.close(self.fd)
                if status:
                    raise AssertionError(f"REPL exited with {status}")
                return
            ready, _, _ = select.select([self.fd], [], [], 0.01)
            if ready:
                try:
                    output = os.read(self.fd, 65536)
                    if b"\x1b[6n" in output:
                        os.write(self.fd, b"\x1b[1;1R")
                except OSError:
                    pass
        os.kill(self.pid, signal.SIGKILL)
        os.waitpid(self.pid, 0)
        os.close(self.fd)
        raise AssertionError("REPL did not exit")


class ReplTerminal(unittest.TestCase):
    def test_edit_completion_and_history_across_sessions(self):
        with tempfile.TemporaryDirectory(prefix="fern-terminal-") as directory:
            history = Path(directory) / "history"
            terminal = Terminal(COMPILER, history)
            try:
                terminal.send(b"4 + 3\x1b[D\x1b[3~2\n")
                terminal.until(b"6 : Int")
                terminal.until(b"fern> ")
                terminal.send(b"printl\t(8)\n")
                terminal.until(b"8\r\n")
                terminal.until(b"fern> ")
                terminal.send(b"40 + 2\n")
                terminal.until(b"42 : Int")
                terminal.until(b"fern> ")
            finally:
                terminal.close()
            self.assertTrue(history.is_file())
            terminal = Terminal(COMPILER, history)
            try:
                terminal.send(b"\x1b[A\n")
                terminal.until(b"42 : Int")
                terminal.until(b"fern> ")
            finally:
                terminal.close()

    def test_nonregular_history_does_not_block_the_terminal(self):
        with tempfile.TemporaryDirectory(prefix="fern-terminal-") as directory:
            history = Path(directory) / "history"
            os.mkfifo(history)
            terminal = Terminal(COMPILER, history)
            try:
                terminal.send(b"20 + 2\n")
                terminal.until(b"22 : Int")
                terminal.until(b"fern> ")
            finally:
                terminal.close()

    def test_interrupt_discards_pending_block_and_keeps_session(self):
        with tempfile.TemporaryDirectory(prefix="fern-terminal-") as directory:
            terminal = Terminal(COMPILER, Path(directory) / "history")
            try:
                terminal.send(b"let saved = 19\n")
                terminal.until(b"fern> ")
                terminal.send(b"fn unfinished():\n")
                terminal.until(b"...   ")
                terminal.send(b"\x03")
                terminal.until(b"fern> ")
                terminal.send(b"saved\n")
                terminal.until(b"19 : Int")
                terminal.until(b"fern> ")
            finally:
                terminal.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fern", type=Path, required=True)
    arguments, remaining = parser.parse_known_args()
    COMPILER = arguments.fern.resolve()
    unittest.main(argv=[__file__, *remaining])
