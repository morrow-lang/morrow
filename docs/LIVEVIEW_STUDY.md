# LiveView source study and Morrow's local preview

Inspected upstream `phoenixframework/phoenix_live_view` at commit
`7f06d34002983f30f44ef24827362a32360a736c` (2026-09-11, “Update assets”).
The shallow checkout is retained at
`/tmp/morrow-liveview-study-rhME9z/source` on the development machine; it is not
part of Morrow's repository or build. No upstream implementation was copied.
LiveView's source retains its upstream MIT license.

## What the source does

- **Mount and reconnect:** the initial HTTP render calls mount and render;
  the connected phase creates a server process and mounts again. Reconnection
  rebuilds that connected view. See [the lifecycle documentation in source](https://github.com/phoenixframework/phoenix_live_view/blob/7f06d34002983f30f44ef24827362a32360a736c/lib/phoenix_live_view.ex#L24).
- **Rendering:** the server diff engine retains rendering fingerprints and
  component state; it produces changes rather than sending a complete page on
  every event. See [the diff engine](https://github.com/phoenixframework/phoenix_live_view/blob/7f06d34002983f30f44ef24827362a32360a736c/lib/phoenix_live_view/diff.ex#L145).
- **Keyed DOM and focus:** the client matches nodes by DOM IDs or generated
  identities, tracks active input selection, and protects locked updates.
  See [DOM patching](https://github.com/phoenixframework/phoenix_live_view/blob/7f06d34002983f30f44ef24827362a32360a736c/assets/js/phoenix_live_view/dom_patch.ts#L167).
- **Events and pending feedback:** event pushes create references associated
  with loading/locking state; acknowledgements release them. Loading is scoped
  to affected controls and persists until outstanding events complete.
  See [push and reply handling](https://github.com/phoenixframework/phoenix_live_view/blob/7f06d34002983f30f44ef24827362a32360a736c/assets/js/phoenix_live_view/view.ts#L1479)
  and [the synchronization guide](https://github.com/phoenixframework/phoenix_live_view/blob/7f06d34002983f30f44ef24827362a32360a736c/guides/client/syncing-changes.md#L47).
- **Form recovery:** reconnect recovery finds matching identified forms and
  sends recovery events before applying the mount patch, avoiding a temporary
  loss of entered values. It checks matching change bindings and guards against
  repeated failed recovery. See [form recovery implementation](https://github.com/phoenixframework/phoenix_live_view/blob/7f06d34002983f30f44ef24827362a32360a736c/assets/js/phoenix_live_view/view.ts#L810).

These are useful interaction contracts, not a claim that Morrow matches LiveView's
framework, ecosystem, diff engine or operational maturity. LiveView supports
client hooks and JavaScript commands; its clients are not devoid of local state.

## The Morrow improvement

The checklist now visibly separates a local draft preview from server-confirmed
tasks. Its typed Morrow `view` computes the preview and UTF-8 byte budget; the
compiled WebAssembly app keeps updating both without a socket. Filters also
remain local. The server's typed actor still owns confirmed tasks.

Submitting displays `Saving…` and `aria-busy` on the add button until the pending
command resolves. It leaves the draft input editable. Only successful transport
admission clears the submitted draft, and subsequent acknowledgements do not
erase the next local draft. A rejected submission retains its text. This applies
LiveView's useful principle of tying feedback to real completion while retaining
Morrow's existing local WASM execution and bounded offline storage.

This does not add optimistic server mutations, a durable offline command queue,
conflict merging, arbitrary form recovery or LiveView compatibility. Existing
uncertain-completion and fresh-incarnation rules still apply.

The independent native and Wasmi traces check Unicode byte counts, offline
submission rejection, loading labels, continued local editing and the 256-byte
limit. The full 100-task, two-held-view oracle remains in place: two additional
feedback nodes produce 511 nodes per view, within the current WASM list bound.
Browser acceptance additionally checks the preview before submission, synchronous
pending feedback, clearing it after acknowledgement, and Unicode edits after a
cold offline reload. Existing focus, offline draft, filter, reconnect and cache
integrity assertions remain unchanged.
