// Per-session serializers — prevents concurrent turns on the same session from
// overlapping. One queue per session; different sessions run concurrently.
const serializers = new Map(); // sessionId → serialize fn

export function serializeFor(sessionId) {
  if (!serializers.has(sessionId)) {
    let q = Promise.resolve();
    serializers.set(sessionId, (fn) => { q = q.then(fn, fn); return q; });
  }
  return serializers.get(sessionId);
}

export function removeSerializer(sessionId) {
  serializers.delete(sessionId);
}
