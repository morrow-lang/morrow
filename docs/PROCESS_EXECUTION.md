# Bounded process execution

Use literal arguments and explicit limits when running a child tool:

```fern
fn main():
    match System.exec_args_bounded(["git", "status", "--short"], 5000, 1048576):
        Ok((status, stdout, stderr)) ->
            println(status)
            println(stdout)
            println(stderr)
        Err(code) -> println("Could not capture process: {code}")
```

The signature is `(List(String), Int, Int) -> Result((Int, String, String), Int)`.
Arguments are passed literally through PATH lookup with the caller's environment;
spaces, quotes and shell metacharacters do not invoke a shell. Executable text
without a valid executable format or interpreter line returns a spawn error;
there is no implicit shell fallback. Standard input is
EOF. Standard output and standard error are captured separately as UTF-8 text.

A normal child exit, including nonzero statuses and 127, returns Ok. The caller
decides whether that exit status means success. Errors return no partial output:

| Code | Meaning |
| --- | --- |
| 1 | Invalid arguments, limits or unsupported SIGCHLD policy |
| 2 | Synchronous spawn error reported by the operating system |
| 3 | Capture deadline reached |
| 4 | Either stream exceeded its independent output limit |
| 5 | Process setup, capture, descriptor or wait failure |
| 6 | Output contains NUL or invalid UTF-8 |
| 7 | Child terminated by a signal |

The timeout is 1–600,000 milliseconds. Each stream can retain at most the supplied
0–16,777,216 bytes; this limit counts UTF-8 bytes, not characters. Exactly the limit
is allowed. There must be 1–4096 arguments and at most 1 MiB of total argument
storage including NUL terminators. The executable name must be nonempty; subsequent
arguments may be empty. Native strings cannot transport an embedded NUL argument.

A slash in the executable name bypasses PATH search. Otherwise PATH allows at
most 1 MiB and 4096 components; an empty component uses the current directory,
and an unset PATH uses `/usr/bin:/bin`. Invalid search configuration returns code 1.
Lookup retains the original argument zero, continues past missing paths and
permission denials, and stops on an invalid executable format.

The first observed failure is retained while cleanup proceeds. A deadline bounds
capture work and initiates cleanup; operating-system spawn, termination and reaping
may extend elapsed return time. POSIX may report a setup/exec failure as a normal
exit 127, so that status cannot be distinguished from a tool intentionally exiting
127 and is not guessed to be a spawn error.

Each call creates a private process group. Completion or failure terminates its
remaining group members before reaping the direct child. Descendants that change
sessions or groups can escape this cleanup; a process group is not containment.
An escaped writer holding an inherited pipe cannot extend capture indefinitely.
This API does not create detached jobs. Parent standard descriptors and signal
policy are preserved; the child starts with an empty signal mask and default
catchable signal dispositions.

Native embedding must retain ordinary SIGCHLD reaping semantics and must not run
a competing reaper or change that policy during the call. SIG_IGN and SA_NOCLDWAIT
are rejected before spawning. The implementation never signals an inherited group
or an identity whose reaping ownership was lost.

The Rust compiler consumes the native heap Result and adapts only a successful
process tuple into its tagged representation. All pointers,
parameters and Result payloads retain 64 bits. Legacy System.exec and
System.exec_args retain their existing compatibility behavior. Interactive process
execution remains unsupported.

On macOS, a group containing only the retained exited child can report EPERM
when signaled. The runtime accepts this only after confirming exit and obtaining
a complete group-membership snapshot containing exactly that child. Failed or
ambiguous snapshots remain errors. There is no such exception for a live child
or on Linux.

## Standard error output

`System.write_stderr(text) -> Result(Unit, Int)` writes exact UTF-8 text to stderr
without adding a newline. `Ok(())` means all bytes were written. Error 1 means
invalid native text, error 2 means more than 16 MiB, and error 3 means IO or signal
setup/restoration failed, or the 65,536-write-attempt budget was exhausted.
Validation happens before output. Empty text succeeds even if stderr is closed.
A later error may follow partial output; writes are not atomic. Blocking OS writes
have no hard deadline. Native strings cannot carry interior NUL bytes.

The function preserves descriptor flags and global signal handlers. It masks
SIGPIPE only in the calling thread, preserves an already pending signal, and
consumes a newly pending write-generated signal after EPIPE before restoring the
mask. Native hosts must not race this operation with competing SIGPIPE consumers,
disposition changes or SIGPIPE injection into the same thread. Other threads'
masks are untouched. REPL native IO remains explicitly unsupported.

Handle a diagnostic write failure while preserving the original command failure:

```fern
fn main() -> Int:
    match System.write_stderr("invalid arguments\n"):
        Ok(()) -> 2
        Err(_) -> 2
```
