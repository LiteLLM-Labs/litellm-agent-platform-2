// Serialized queue — prevents concurrent agent config writes from racing.
export const serialize = (() => {
  let q = Promise.resolve();
  return (fn) => { q = q.then(fn, fn); return q; };
})();

// Map of sessionId → active ChildProcess (or null when idle).
export const activeProcesses = new Map();
