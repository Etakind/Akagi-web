// Three-way merge for "draft being edited in the UI" vs "config changed
// externally".
//
// Both the Bots page's cloud-inference card and the Setup wizard hold an
// editable draft seeded from the stored config. The stored config can change
// underneath them while the draft is open — most notably when a purchased API
// key finishes delivery with the purchase dialog unmounted and the purchase
// another view saves configuration. Blindly re-seeding the draft would wipe
// the user's unsaved edits; ignoring the change would leave a stale draft
// whose Save silently reverts the externally-written values (e.g. a paid key).
//
// `mergeExternal(draft, prevStored, nextStored)` resolves this per field:
// - fields the user did NOT touch (draft still equals `prevStored`, the
//   stored snapshot the draft was last synced against) adopt the new stored
//   value;
// - fields the user DID edit keep their draft value, even when the stored
//   value moved too — the in-progress edit wins the conflict;
// - plain objects are merged recursively, so nested sections (e.g. `capture.chromium`)
//   merge field-by-field instead of all-or-nothing.
//
// Values are compared structurally (via JSON), which is exact for config
// shapes (primitives, arrays, plain objects). Arrays are treated as leaves.

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true
  return JSON.stringify(a) === JSON.stringify(b)
}

export function mergeExternal<T>(draft: T, prevStored: T, nextStored: T): T {
  // No external change in this subtree — keep the draft as-is.
  if (deepEqual(prevStored, nextStored)) return draft
  // The user never touched this subtree — adopt the external change whole.
  if (deepEqual(draft, prevStored)) return nextStored
  // Both changed: split the conflict per key when possible.
  if (isPlainObject(draft) && isPlainObject(prevStored) && isPlainObject(nextStored)) {
    const out: Record<string, unknown> = {}
    for (const k of new Set([...Object.keys(draft), ...Object.keys(nextStored)])) {
      out[k] = mergeExternal(draft[k], prevStored[k], nextStored[k])
    }
    return out as T
  }
  // Conflicting leaf edit: what the user typed wins.
  return draft
}
