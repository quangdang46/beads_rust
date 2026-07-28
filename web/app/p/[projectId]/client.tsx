"use client";

import { AppShell } from "@/components/app-shell";
import { use } from "react";

/**
 * Client wrapper that receives params from the server page component
 * and unwraps the Promise for client-side rendering.
 */
export function AppShellClientWrapper({
  params,
}: {
  params: Promise<{ projectId: string }>;
}) {
  const { projectId } = use(params);
  return <AppShell projectId={projectId} />;
}
