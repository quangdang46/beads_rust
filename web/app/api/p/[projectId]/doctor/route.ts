import "server-only";
import { NextRequest, NextResponse } from "next/server";
import { runBr, handleError, buildMeta } from "@/lib/br";

// ---------------------------------------------------------------------------
// GET /api/p/[projectId]/doctor — check br version
//
// Returns a DoctorResponse-like shape that the frontend (api-client.ts)
// expects for the doctor endpoint.
// ---------------------------------------------------------------------------
export async function GET(
  _req: NextRequest,
  { params }: { params: Promise<{ projectId: string }> },
) {
  try {
    const { projectId } = await params;

    // `br version --json` returns version info.
    // `br version -s` returns just the version string.
    let version: string | undefined;
    try {
      version = await runBr(["version", "-s"], { raw: true, cwd: projectId, timeout: 5000 });
    } catch {
      version = undefined;
    }

    const meta = buildMeta();

    return NextResponse.json({
      kind: "bd" as const,
      ok: !!version,
      version,
      repoPath: projectId,
      message: version
        ? `br ${version}`
        : "br CLI not found or not responding",
      project: {
        id: projectId,
        name: projectId.split(/[\\/]/).pop() || projectId,
        path: projectId || null,
      },
      config: {
        humanActor: meta.humanActor,
        humanAllowlist: meta.humanAllowlist,
        pollIntervalMs: meta.pollIntervalMs,
      },
    });
  } catch (err) {
    return handleError(err);
  }
}
