import "server-only";
import { NextRequest, NextResponse } from "next/server";
import { runBrJson, handleError } from "@/lib/br";

// ---------------------------------------------------------------------------
// GET /api/projects — return auto-detected project info
//
// Uses `br info --json` to discover the current workspace and its metadata.
// The frontend expects { projects: ProjectInfo[] }.
// ---------------------------------------------------------------------------
export async function GET(_req: NextRequest) {
  try {
    const info = await runBrJson<{
      beads_dir: string;
      database_path: string;
      config?: {
        issue_prefix?: string;
      };
      mode: string;
    }>(["info"]);

    // Derive a project id and name from the beads directory.
    const beadsDir = info.beads_dir ?? "";
    // The parent directory of .beads is the repo root.
    const repoPath = beadsDir.replace(/[\\/]\.beads$/, "");
    const name = repoPath.split(/[\\/]/).filter(Boolean).pop() || "unknown";
    const id = info.config?.issue_prefix || name;

    const project = {
      id,
      name,
      path: repoPath || null,
      hasBeads: true,
    };

    return NextResponse.json({ projects: [project] });
  } catch (err) {
    return handleError(err);
  }
}
