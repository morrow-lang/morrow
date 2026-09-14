# Actor-owned deferred cleanup

`defer` belongs to its source function activation. A receiving actor can register
cleanup, suspend across messages or loops, call another helper, and retain that
cleanup until the function actually returns. Generated physical callbacks do not
change this lifetime.

The native runtime owns a rooted stack of logical scopes for each actor. Entering
a source function that owns `defer` pushes one scope. Each executed `defer`
registers a closure containing that execution's lexical snapshots. Registration
is atomic and does not itself yield. Dynamic registrations inside loops belong
to the function, not the iteration.

On normal return, the compiler evaluates and roots the return value first, drains
the current scope in reverse registration order, then resumes the caller with
that value. A tail caller with cleanup retains its return continuation: its
cleanup runs after the callee's cleanup and return. Ordinary functions retain
their existing native cleanup behavior when called synchronously.

A runtime fault drains every active logical scope, inner first. A failed cleanup
does not prevent older cleanup from running. The original body fault wins; if
there was no earlier fault, the first cleanup fault wins. Supervision observes
the final preserved fault only after cleanup finishes.

Explicit native host cancellation (`fern_managed_close`/`fern_managed_stop`)
marks the session stopped, drains all admitted actor cleanup, and then retires
actor roots. Queued source work and suspended receive bodies do not resume.
Cancellation does not restart actors. A pre-existing invocation fault retains
precedence over cleanup failures. Closing an already closed host handle has no
further effect. The REPL exposes the same lifecycle through `Session::stop_actors`.

Scopes and registered callbacks share a limit of 4096 entries per actor. Their
retained graphs also count against the invocation byte budget. A rejected
registration publishes nothing; previously admitted callbacks still drain.
Foreign pointer graphs remain forbidden in managed frames. Cleanup closures and
saved return values remain rooted through collector safepoints and are released
after their owning scopes end.

Cleanup adapters invoke the original zero-argument Unit cleanup function using
its correct native effect ABI. Cleanup bodies retain the language's ordinary
synchronous cleanup execution; they are not independently scheduled actor
bodies. Existing restrictions on receiving/deferred actor effects still apply.
No native caller stack is retained while the actor is suspended.

Acceptance is in `crates/fern/tests/cranelift_backend.rs`,
`crates/fern/tests/repl_actors.rs`, and
`crates/fern-runtime/src/managed/cleanup.rs`. Native oracles cover receive and tail
lifetimes, dynamic loop snapshots, Unicode return roots, nested cleanup failures,
cancellation, and admission limits. The runtime simulation compares 64 seeded
256-operation scope histories with an independent LIFO model, forces precise
collection after each operation, checks first-failure precedence, and verifies
retained-byte accounting returns to baseline. Empty-scope exhaustion and the
4095-callback plus one-scope boundary have independent bounded tests.
