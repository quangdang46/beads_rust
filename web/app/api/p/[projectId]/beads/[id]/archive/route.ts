import "server-only";
import { NextRequest, NextResponse } from "next/server";
import { runBrJson, fetchBead, handleError } from "@/lib/br";
import { beadSchema } from "@/lib/schema";
import type { Bead } from "@/lib/schema";

// ---------------------------------------------------------------------------
// POST /api/p/[projectId]/beads/[id]/archive — close + label "archived"
//
// 1. br label add <id> -l "archived"
// 2. br close <id>
// Returns the updated bead.
// ---------------------------------------------------------------------------
export async function POST(
  _req: NextRequest,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  try {
    const { projectId, id } = await params;

    // Add "archived" label first.
    await runBrJson(["label", "add", id, "-l", "archived"], {
      cwd: projectId,
    });

    // Then close.
    await runBrJson(["close", id], { cwd: projectId });

    // Re-fetch the full bead.
    const raw = await fetchBead(id, { cwd: projectId });
    const bead: Bead = beadSchema.parse(raw);
    return NextResponse.json(bead);
  } catch (err) {
    return handleError(err);
  }
}
