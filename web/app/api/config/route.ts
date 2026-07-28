import "server-only";
import { NextRequest, NextResponse } from "next/server";
import { buildMeta, handleError } from "@/lib/br";

// ---------------------------------------------------------------------------
// GET /api/config — read config
//
// Returns the config section of a DoctorResponse (the frontend calls
// this via api.saveConfig() with method: "PUT" to patch config).
// ---------------------------------------------------------------------------
export async function GET() {
  try {
    return NextResponse.json(buildMeta());
  } catch (err) {
    return handleError(err);
  }
}

// ---------------------------------------------------------------------------
// PUT /api/config — update config (stub)
//
// Currently a no-op that accepts a patch body and returns the current config.
// In the future this could persist to a config file or env.
// ---------------------------------------------------------------------------
export async function PUT(req: NextRequest) {
  try {
    // Accept any JSON body (future: validate and persist).
    await req.json().catch(() => ({}));
    return NextResponse.json(buildMeta());
  } catch (err) {
    return handleError(err);
  }
}
