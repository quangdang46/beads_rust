import "server-only";
import { NextRequest, NextResponse } from "next/server";
import { runBrJson, fetchBead, handleError } from "@/lib/br";
import { beadSchema } from "@/lib/schema";
import type { Bead } from "@/lib/schema";

// ---------------------------------------------------------------------------
// POST /api/p/[projectId]/beads/[id]/status — set status (or close)
//
// Body: { status: string, reason?: string }
// If status is "closed", uses br close with optional --reason.
// Otherwise uses br update -s <status>.
// ---------------------------------------------------------------------------
export async function POST(
  req: NextRequest,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  try {
    const { projectId, id } = await params;
    const body = await req.json();
    const { status, reason } = body as { status?: string; reason?: string };

    if (!status) {
      return NextResponse.json({ error: "status is required" }, { status: 400 });
    }

    if (status === "closed") {
      const args: string[] = ["close", id];
      if (reason) {
        args.push("-r", reason);
      }
      await runBrJson(args, { cwd: projectId });
    } else {
      const args: string[] = ["update", id, "-s", status];
      if (reason && status === "closed") {
        args.push("-r", reason);
      }
      await runBrJson(args, { cwd: projectId });
    }

    // Re-fetch and return the full bead.
    const raw = await fetchBead(id, { cwd: projectId });
    const bead: Bead = beadSchema.parse(raw);
    return NextResponse.json(bead);
  } catch (err) {
    return handleError(err);
  }
}
