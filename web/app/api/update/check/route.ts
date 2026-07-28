import "server-only";
import { NextResponse } from "next/server";
import { handleError } from "@/lib/br";

// ---------------------------------------------------------------------------
// GET /api/update/check — check for updates (stub)
//
// Returns an UpdateStatus shape matching what the frontend expects.
// This is a stub since br is not a git-based project that does
// self-update via git pull. Returns "up to date" always.
// ---------------------------------------------------------------------------
export async function GET() {
  try {
    return NextResponse.json({
      isGitRepo: false,
      supervised: false,
      behind: 0,
      localSha: "",
      remoteSha: "",
    });
  } catch (err) {
    return handleError(err);
  }
}
