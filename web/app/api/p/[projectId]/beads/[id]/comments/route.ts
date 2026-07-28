import "server-only";
import { NextRequest, NextResponse } from "next/server";
import { runBr, fetchBead, handleError } from "@/lib/br";
import { beadSchema } from "@/lib/schema";
import type { Bead } from "@/lib/schema";

// ---------------------------------------------------------------------------
// POST /api/p/[projectId]/beads/[id]/comments — add a comment
//
// Body: { text: string }
// br comments add <id> <text> returns the comment object as JSON.
// After adding, we re-fetch the full bead to return updated state.
// ---------------------------------------------------------------------------
export async function POST(
  req: NextRequest,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  try {
    const { projectId, id } = await params;
    const body = await req.json();
    const { text } = body as { text?: string };

    if (!text || typeof text !== "string" || !text.trim()) {
      return NextResponse.json({ error: "text is required" }, { status: 400 });
    }

    // br comments add <id> <text> -- accepts text as positional arguments.
    // Use -m to pass the text safely (avoids shell splitting).
    await runBr(["comments", "add", id, "-m", text], { cwd: projectId });

    // Re-fetch the full bead (comments are included in show output).
    const raw = await fetchBead(id, { cwd: projectId });
    const bead: Bead = beadSchema.parse(raw);
    return NextResponse.json(bead);
  } catch (err) {
    return handleError(err);
  }
}
