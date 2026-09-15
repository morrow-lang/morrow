# Morrow live protobuf v1

The first-party live application negotiates WebSocket subprotocol
`morrow.live.protobuf.v1` and transfers one binary protobuf Envelope per WebSocket
message. The logical application `version` remains 1. The
[schema](morrow_wire_v1.proto) defines field numbers and protobuf scalar types; the
Rust `morrow-web-protocol::binary` implementation enforces this additional closed
profile. Generated general-purpose protobuf decoders alone do not enforce it.

## Closed profile

- An envelope has its explicit `kind` and exactly one corresponding payload.
  Unknown fields, duplicate non-repeated fields, unknown kinds/statuses/errors and
  irrelevant variant fields are rejected. Declared repeated task fields may recur.
- Every selected payload field is required, including scalar `false` and `0`.
  `Join.resume_namespace` is the exception: absence means no resume request. Empty
  task lists must still have a present `TaskList` message, which may be zero bytes.
  Optional protobuf declarations retain presence rather than inventing defaults.
- Wire scalar types must match the schema. Booleans are precisely 0 or 1, versions
  must fit an unsigned byte, and unsigned kind/status values must fit uint32 before
  registry validation. Strings must contain valid UTF-8. All language integers use
  signed 64-bit ZigZag (`sint64`), including values outside JavaScript's exact-number
  range. Generated JavaScript clients must preserve 64-bit values, for example as
  BigInt; conversion through Number is not permitted.
- Frames are at most 65,536 bytes. Identity fields are at most 128 UTF-8 bytes;
  task/mutation labels at most 256; task lists at most 100 entries. These limits
  are checked by an allocation-free structural scan before owned DTO decoding.
  Input nesting is capped at 16; the current schema itself is acyclic and shallower.
  Encoder source field/cardinality limits precede DTO cloning; exact encoded size
  precedes encoded-buffer allocation.
- Field order may vary and bounded, valid non-minimal protobuf varints are accepted.
  This is not a canonical-byte format. Replay/deduplication compares typed command
  content and authority, never a hash of arbitrary incoming bytes.
- Structural errors produce `Malformed`; an oversized whole frame produces
  `FrameTooLarge`. Authority, positive identifiers, valid labels, supported
  application versions, revisions and namespace semantics are still checked by
  Hub/Client after decoding. The codec is not an authorization boundary.

## Registries

| Envelope kind | Required payload field |
| ---: | --- |
| 1 | Join, field 2 |
| 2 | Command, field 3 |
| 3 | Connected, field 4 |
| 4 | Snapshot, field 5 |
| 5 | Reset, using Snapshot field 5 |
| 6 | Outcome, field 6 |
| 7 | Error name, field 7 |

Mutation kind 1 requires only label (Add); kind 2 requires ID and an explicitly
present boolean (SetDone); kind 3 requires only ID (Remove). The kind field itself
is always required. Outcome statuses are 1 Applied, 2 Conflict, 3 NotFound,
4 Capacity and 5 Unknown.

Error names are `malformed`, `frame_too_large`, `version_mismatch`,
`invalid_identity`, `invalid_label`, `invalid_limits`, `room_limit`,
`namespace_limit`, `connection_limit`, `namespace_expired`, `connection_expired`,
`unauthorized`, `incarnation_mismatch`, `payload_mismatch`, `sequence_gap`,
`exhausted`, `time_regression`, `offline`, `pending`, `resync_required`,
`unexpected_outcome` and `stale_snapshot`.

Published field numbers, variant numbers and error names are never reassigned.
Because v1 rejects unknown fields, adding fields or variants requires an explicitly
negotiated new schema version rather than relying on generic protobuf's unknown-field
behavior. An explicitly negotiated JSON compatibility path may remain available;
receivers must not guess the format or silently switch after a malformed frame.
JSON continues to serve separate HTTP and persisted-record contracts.

Peer framing has its own negotiated protocol and envelope. It carries bare Command
protobuf messages or complete server Envelopes as bounded bytes, using the same
application registry. These messages transfer values, not native heaps or remote
language process handles.
