import "server-only";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import * as path from "node:path";
import * as fs from "node:fs";
import { NextResponse } from "next/server";

const execFileAsync = promisify(execFile);

function resolveBrBin(): string {
  return process.env.BR_BIN || "br";
}

/**
 * Walk up from `dir` to find the first ancestor (or `dir` itself) that
 * contains a `.beads` directory. Falls back to the parent of `cwd` (the
 * typical Next.js workspace layout where `web/` is under the project root).
 */
function resolveBeadsRoot(dir = process.cwd()): string {
  let current = path.resolve(dir);
  for (let i = 0; i < 20; i++) {
    if (fs.existsSync(path.join(current, ".beads"))) return current;
    const parent = path.dirname(current);
    if (parent === current) break;
    current = parent;
  }
  // Fallback: parent of web/ is the typical project root.
  const parent = path.dirname(process.cwd());
  if (fs.existsSync(path.join(parent, ".beads"))) return parent;
  return process.cwd();
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

export class BrError extends Error {
  constructor(
    message: string,
    public readonly code: number | null,
    public readonly stderr: string,
  ) {
    super(message);
    this.name = "BrError";
  }
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export interface BrOptions {
  cwd?: string;
  timeout?: number;
  /** Extra environment variables to merge into `process.env`. */
  env?: Record<string, string>;
  /** When true, do NOT append `--json` to args. */
  raw?: boolean;
}

// ---------------------------------------------------------------------------
// CLI wrappers
// ---------------------------------------------------------------------------

/**
 * Run the `br` CLI binary and return stdout as a string.
 * Automatically appends `--json` unless `opts.raw` is true.
 */
export async function runBr(
  args: string[],
  opts: BrOptions = {},
): Promise<string> {
  const bin = resolveBrBin();
  const cwd = opts.cwd ?? resolveBeadsRoot();
  const finalArgs = opts.raw
    ? args
    : args.includes("--json")
      ? args
      : [...args, "--json"];

  try {
    const { stdout } = await execFileAsync(bin, finalArgs, {
      cwd,
      timeout: opts.timeout ?? 30000,
      env: { ...process.env, ...opts.env } as NodeJS.ProcessEnv,
      maxBuffer: 10 * 1024 * 1024,
    });
    return stdout.trim();
  } catch (err: unknown) {
    if (err && typeof err === "object" && "stderr" in err) {
      const e = err as {
        code?: number;
        stderr?: string | Buffer;
        message?: string;
      };
      const stderr = Buffer.isBuffer(e.stderr)
        ? e.stderr.toString()
        : (e.stderr ?? "");
      throw new BrError(
        e.message || `br command failed: ${finalArgs.join(" ")}`,
        e.code ?? null,
        stderr.trim(),
      );
    }
    throw err;
  }
}

/**
 * Run a br command that produces JSON and parse it.
 */
export async function runBrJson<T = unknown>(
  args: string[],
  opts: BrOptions = {},
): Promise<T> {
  const out = await runBr(args, opts);
  if (!out) return [] as unknown as T;
  try {
    return JSON.parse(out) as T;
  } catch {
    throw new BrError(
      `Failed to parse br JSON output: ${out.slice(0, 200)}`,
      null,
      "",
    );
  }
}

/**
 * Call `br show <id> --json`, parse the single-element array, and return the
 * bead object. Throws if the array is empty.
 */
export async function fetchBead(
  id: string,
  opts: BrOptions = {},
): Promise<Record<string, unknown>> {
  const arr = await runBrJson<Record<string, unknown>[]>(["show", id], opts);
  if (!Array.isArray(arr) || arr.length === 0) {
    throw new BrError(`Bead not found: ${id}`, 1, "");
  }
  return arr[0];
}

// ---------------------------------------------------------------------------
// Shared error handler for API routes
// ---------------------------------------------------------------------------

export function handleError(err: unknown): NextResponse {
  if (err instanceof BrError) {
    const status = err.code === 1 ? 400 : err.code && err.code >= 2 ? 500 : 500;
    return NextResponse.json(
      { error: err.message, stderr: err.stderr, code: err.code },
      { status },
    );
  }
  if (
    err &&
    typeof err === "object" &&
    "status" in err &&
    typeof (err as Record<string, unknown>).status === "number"
  ) {
    const e = err as { status: number; message?: string };
    return NextResponse.json(
      { error: e.message ?? "Request error" },
      { status: e.status },
    );
  }
  const msg = err instanceof Error ? err.message : "Internal server error";
  return NextResponse.json({ error: msg }, { status: 500 });
}

// ---------------------------------------------------------------------------
// Meta helper for BeadsResponse
// ---------------------------------------------------------------------------

export interface RouteMeta {
  kind: "bd";
  humanActor: string;
  humanAllowlist: string[];
  pollIntervalMs: number;
  gamification?: boolean;
}

export function buildMeta(): RouteMeta {
  return {
    kind: "bd",
    humanActor: process.env.HUMAN_ACTOR || "human",
    humanAllowlist: (process.env.HUMAN_ALLOWLIST || "")
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean),
    pollIntervalMs: Number(process.env.POLL_INTERVAL_MS) || 30000,
    gamification: true,
  };
}
