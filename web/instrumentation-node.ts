/**
 * Node-only server hardening (imported by instrumentation.ts under the Node
 * runtime). Top-level side effects install the handlers on import.
 *
 * The SSE change stream (app/api/p/[projectId]/beads/stream) writes to a
 * long-lived socket. When a browser tab closes or reloads mid-write, the
 * runtime's flush to that dead socket can throw EPIPE/ECONNRESET *asynchronously*
 * — outside our try/catch — surfacing as an uncaughtException that takes down the
 * whole dev server. That crash made in-flight requests (e.g. saving a bead edit)
 * appear to silently fail. These are normal network teardowns, not app bugs, so
 * swallow exactly those and let every other error crash as usual.
 */
import { closeAllSseStreams } from "./lib/sse-registry";

/**
 * `next start` drains in-flight requests on SIGINT/SIGTERM before exiting, and
 * the SSE stream above never completes on its own — so with a browser tab open
 * the server would wait forever and Ctrl+C would appear to do nothing. Close
 * the streams so the drain can finish. We deliberately don't call
 * process.exit(): Next's own handler still owns the actual shutdown.
 */
let closing = false;
const closeStreams = (sig: string) => {
  if (closing) return;
  closing = true;
  const n = closeAllSseStreams();
  if (n > 0) console.log(`[instrumentation] ${sig}: closed ${n} open SSE stream(s)`);
};
process.on("SIGINT", () => closeStreams("SIGINT"));
process.on("SIGTERM", () => closeStreams("SIGTERM"));

const isBenignNetError = (e: unknown): boolean => {
  const code = (e as NodeJS.ErrnoException | undefined)?.code;
  return code === "EPIPE" || code === "ECONNRESET" || code === "ERR_STREAM_DESTROYED";
};

process.on("uncaughtException", (err) => {
  if (isBenignNetError(err)) {
    console.warn(`[instrumentation] ignored benign network error: ${err.message}`);
    return;
  }
  // Not ours to swallow — re-throw to preserve Node's default crash behavior.
  throw err;
});

process.on("unhandledRejection", (reason) => {
  if (isBenignNetError(reason)) {
    console.warn("[instrumentation] ignored benign network rejection");
    return;
  }
  throw reason;
});
