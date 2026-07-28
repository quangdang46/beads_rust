"use client";

import { useParams } from "next/navigation";
import { AppShell } from "@/components/app-shell";

export default function ProjectPage() {
  const { projectId } = useParams<{ projectId: string }>();
  return <AppShell projectId={projectId} />;
}
