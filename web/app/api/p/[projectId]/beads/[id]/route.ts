import "server-only";
import { NextRequest, NextResponse } from "next/server";
import {
  runBrJson,
  fetchBead,
  handleError,
} from "@/lib/br";
import {
  beadSchema,
  updateInputSchema,
} from "@/lib/schema";
import type { Bead } from "@/lib/schema";

// ---------------------------------------------------------------------------
// GET /api/p/[projectId]/beads/[id] — show single bead
// ---------------------------------------------------------------------------
export async function GET(
  _req: NextRequest,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  try {
    const { projectId, id } = await params;
    const raw = await fetchBead(id, { cwd: projectId });
    const bead: Bead = beadSchema.parse(raw);
    return NextResponse.json(bead);
  } catch (err) {
    return handleError(err);
  }
}

// ---------------------------------------------------------------------------
// PATCH /api/p/[projectId]/beads/[id] — update a bead
//
// br update accepts: --title, -d/--description, -p/--priority, -a/--assignee,
//                     -s/--status, -t/--type, --add-label, --remove-label,
//                     --set-labels / --labels, --parent
// ---------------------------------------------------------------------------
export async function PATCH(
  req: NextRequest,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  try {
    const { projectId, id } = await params;
    const body = await req.json();
    const input = updateInputSchema.parse(body);

    const args: string[] = ["update", id];

    if (input.title !== undefined) {
      args.push("--title", input.title);
    }
    if (input.description !== undefined) {
      args.push("-d", input.description);
    }
    if (input.priority !== undefined) {
      args.push("-p", String(input.priority));
    }
    if (input.assignee !== undefined) {
      args.push("--assignee", input.assignee);
    }
    if (input.status !== undefined) {
      // br update now accepts status changes for non-terminal states.
      // Closed/deleted must use the dedicated commands.
      args.push("-s", input.status);
    }
    if (input.issue_type !== undefined) {
      args.push("-t", input.issue_type);
    }
    if (input.labels !== undefined) {
      // Replace all labels.
      for (const l of input.labels) {
        args.push("--set-labels", l);
      }
    }
    if (input.parent !== undefined) {
      args.push("--parent", input.parent === "" ? "" : input.parent);
    }

    // If no meaningful changes were provided, just return the current bead.
    if (args.length === 2) {
      const raw = await fetchBead(id, { cwd: projectId });
      const bead: Bead = beadSchema.parse(raw);
      return NextResponse.json(bead);
    }

    // br update --json returns a single-element array.
    const result = await runBrJson<Record<string, unknown>[]>(args, {
      cwd: projectId,
    });

    // Re-fetch the full bead to return a complete object.
    const full = await fetchBead(id, { cwd: projectId });
    const bead: Bead = beadSchema.parse(full);
    return NextResponse.json(bead);
  } catch (err) {
    return handleError(err);
  }
}

// ---------------------------------------------------------------------------
// DELETE /api/p/[projectId]/beads/[id] — delete a bead
// ---------------------------------------------------------------------------
export async function DELETE(
  _req: NextRequest,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  try {
    const { projectId, id } = await params;
    const result = await runBrJson<{ deleted: string[] }>(
      ["delete", id, "--force"],
      { cwd: projectId },
    );
    return NextResponse.json({ deleted: result.deleted?.[0] ?? id });
  } catch (err) {
    return handleError(err);
  }
}
