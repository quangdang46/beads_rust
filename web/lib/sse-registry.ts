/**
 * Registry of open SSE streams, so shutdown can close them.
 *
 * `next start` drains in-flight requests before exiting. An SSE stream never
 * completes on its own, so without this the server hangs on SIGINT/SIGTERM
 * for as long as a browser tab is open. instrumentation-node.ts closes
 * everything registered here when a shutdown signal arrives.
 *
 * Keyed off `globalThis` rather than module scope: route handlers and
 * instrumentation can land in separate bundles, and a plain module-level Set
 * would then give each its own copy.
 */
type Closer = () => void;

const KEY = Symbol.for("scotty.sse.openStreams");

function registry(): Set<Closer> {
  const g = globalThis as unknown as Record<symbol, Set<Closer> | undefined>;
  const existing = g[KEY];
  if (existing) return existing;
  const created = new Set<Closer>();
  g[KEY] = created;
  return created;
}

/** Register an open stream's close fn. Returns an unregister fn. */
export function registerSseStream(close: Closer): () => void {
  const r = registry();
  r.add(close);
  return () => {
    r.delete(close);
  };
}

/** Close every open stream. Returns how many were closed. */
export function closeAllSseStreams(): number {
  const r = registry();
  const closers = [...r];
  r.clear();
  for (const close of closers) {
    try {
      close();
    } catch {
      /* a stream torn down concurrently is fine */
    }
  }
  return closers.length;
}
