+++
schema_version = 1
id = "01M2XHZ95QGN8BQK045F70RHCK"
title = "Actor-based concurrency model"
date = "2026-01-27"
status = "accepted"
tags = ["runtime", "ai"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: ✅ Adopted
* **Decision**: I will use an actor-based concurrency model (Erlang/Elixir style) instead of threads, async/await, or channels.
* **Context**: Considered several options: (1) OS threads - too heavy, difficult to reason about shared state, (2) async/await - complex, color functions, can't block, (3) Go-style goroutines with channels - better but still allows shared memory bugs, (4) Actor model - isolated processes with message passing only, no shared memory, supervisor trees for fault tolerance. The actor model provides the best balance of safety and expressiveness. Even though it adds ~500KB to binary size, this is acceptable given the safety and capability benefits (can replace Redis, RabbitMQ in many cases).
* **Consequences**: Need to implement a lightweight process scheduler, message queues, and supervision trees. All concurrent code uses message passing. The runtime will be slightly larger but provides superior reliability.
