import "server-only";
import { NextRequest, NextResponse } from "next/server";
import { runBr, runBrJson, fetchBead, buildMeta, handleError } from "@/lib/br";
import { createInputSchema, beadArraySchema, beadSchema, unwrapEnvelope } from "@/lib/schema";
import type { Bead } from "@/lib/schema";

// ---------------------------------------------------------------------------
// GET /api/p/[projectId]/beads — list all beads
// ---------------------------------------------------------------------------
export async function GET(
  _req: NextRequest,
  { params }: { params: Promise<{ projectId: string }> },
) {
  try {
    const { projectId } = await params;

    // `br export --json -f json` outputs a JSON array of issues.
    // Use `--all` to include closed issues (matches the old board behaviour).
    const raw = await runBr(["export", "--json", "-f", "json", "--all"], {
      cwd: projectId,
    });

    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      return NextResponse.json(
        { error: "Failed to parse br export output as JSON", beads: [], meta: buildMeta() },
        { status: 500 },
      );
    }

    // Unwrap envelope if present (--json global flag may wrap in { schema_version, data }).
    const data = unwrapEnvelope(parsed);

    // Validate with beadArraySchema (which allows unknown fields and strips extras).
    const beads: Bead[] = beadArraySchema.parse(data ?? []);

    return NextResponse.json({
      beads,
      meta: buildMeta(),
    });
  } catch (err) {
    return handleError(err);
  }
}

// ---------------------------------------------------------------------------
// POST /api/p/[projectId]/beads — create a new bead
// ---------------------------------------------------------------------------
export async function POST(
  req: NextRequest,
  { params }: { params: Promise<{ projectId: string }> },
) {
  try {
    const { projectId } = await params;
    const body = await req.json();
    const input = createInputSchema.parse(body);

    const args: string[] = ["create", input.title];
    args.push("-t", input.issue_type);
    args.push("-p", String(input.priority));

    if (input.description) {
      args.push("-d", input.description);
    }
    if (input.assignee) {
      args.push("-a", input.assignee);
    }
    for (const label of input.labels) {
      args.push("-l", label);
    }
    if (input.parent) {
      args.push("--parent", input.parent);
    }

    // Backlog = start deferred.
    if (input.backlog) {
      args.push("-s", "deferred");
    }

    // br create returns a JSON object with { id, title, status, ... }.
    const created = await runBrJson<{ id: string }>(args, { cwd: projectId });

    if (!created.id) {
      return NextResponse.json({ error: "br create did not return an id" }, { status: 500 });
    }

    // Fetch the full bead to return a complete Bead object.
    const full = await fetchBead(created.id, { cwd: projectId });
    const bead: Bead = beadSchema.parse(full);
    return NextResponse.json(bead, { status: 201 });
  } catch (err) {
    return handleError(err);
  }
}
