import "server-only";
import { NextRequest, NextResponse } from "next/server";
import { runBrJson, fetchBead, handleError } from "@/lib/br";
import { beadSchema, DEP_TYPES } from "@/lib/schema";
import type { Bead, DepType } from "@/lib/schema";

// ---------------------------------------------------------------------------
// POST /api/p/[projectId]/beads/[id]/deps — add a dependency
//
// Body: { depends_on_id: string, type: DepType }
// br dep add <id> <depends_on_id> -t <type>
// ---------------------------------------------------------------------------
export async function POST(
  req: NextRequest,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  try {
    const { projectId, id } = await params;
    const body = await req.json();
    const { depends_on_id, type } = body as {
      depends_on_id?: string;
      type?: string;
    };

    if (!depends_on_id) {
      return NextResponse.json(
        { error: "depends_on_id is required" },
        { status: 400 },
      );
    }

    const depType: string =
      type && DEP_TYPES.includes(type as DepType) ? type : "blocks";

    await runBrJson(["dep", "add", id, depends_on_id, "-t", depType], {
      cwd: projectId,
    });

    // Re-fetch the full bead.
    const raw = await fetchBead(id, { cwd: projectId });
    const bead: Bead = beadSchema.parse(raw);
    return NextResponse.json(bead);
  } catch (err) {
    return handleError(err);
  }
}

// ---------------------------------------------------------------------------
// DELETE /api/p/[projectId]/beads/[id]/deps — remove a dependency
//
// Body: { depends_on_id: string }
// br dep remove <id> <depends_on_id>
// ---------------------------------------------------------------------------
export async function DELETE(
  req: NextRequest,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  try {
    const { projectId, id } = await params;
    const body = await req.json();
    const { depends_on_id } = body as { depends_on_id?: string };

    if (!depends_on_id) {
      return NextResponse.json(
        { error: "depends_on_id is required" },
        { status: 400 },
      );
    }

    await runBrJson(["dep", "remove", id, depends_on_id], {
      cwd: projectId,
    });

    // Re-fetch the full bead.
    const raw = await fetchBead(id, { cwd: projectId });
    const bead: Bead = beadSchema.parse(raw);
    return NextResponse.json(bead);
  } catch (err) {
    return handleError(err);
  }
}
