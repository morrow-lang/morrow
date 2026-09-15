# Web actor workers

`morrow-web` routes each room to one pinned OS thread. `MORROW_WEB_WORKERS`
accepts 1 through 32; the default is available CPU parallelism capped at four.
Each worker constructs, runs and drops its own compiled Morrow actor runtime and
heaps. Native Morrow pointers and actor IDs never cross threads. The network
executor exchanges bounded Rust messages with the workers.

Room placement is FNV-1a over the room name modulo the configured worker count.
The socket stores its private route after joining; command payloads cannot select
a different worker. A room's commands remain ordered on its owner. Moving a
socket between rooms waits asynchronously for its old connection to disconnect,
so a move cannot temporarily exceed the global connection cap. Rooms sharing a
worker still share that worker's execution time; there is no live room migration
or work stealing.

Authentication is one separate asynchronous owner. Each admitted session carries
a revocable, expiring capability checked by the worker before each join or
command. Logout revokes it immediately without waiting for Morrow execution. A
command already executing may finish; queued commands are rejected after
revocation. This is an admission boundary, not cancellation of an in-flight
application effect.

All workers share one 256-request ingress budget, counting queued and currently
executing requests, including authentication. Saturation rejects admission;
adding workers does not multiply the budget. Rooms, retained command namespaces
and physical protocol connections hold shared RAII leases. Rollback, expiry and
destruction release those leases. Authentication sessions, HTTP requests, TCP
connections and WebSocket admissions also retain their existing process-wide
limits. Per-room task, dedupe and subscriber limits remain unchanged.

With `MORROW_WEB_DATA_DIR`, workers share a Rust checkpoint writer and its exclusive
filesystem lock. Only owned Rust records and file handles are shared. The writer
serializes durable commits; a slow filesystem may therefore delay commits on
other workers, while authentication and the network executor stay responsive.
Checkpoints use room names independent of worker assignment, allowing the worker
count to change after restart. Restart still creates fresh authentication,
incarnations and command namespaces; this does not promise exactly-once effects
across restarts or multiple nodes.

The worker tests hold one non-`Send` domain callback at a condition-variable gate
and require another worker's command and global logout to complete before
releasing it. They also verify that revoked queued commands never enter the
domain, cross-worker moves work at the global connection cap, and worker count
does not increase room or ingress admission. Protocol tests independently verify
shared quota rollback, reconnect transfer, expiry and owner destruction.
