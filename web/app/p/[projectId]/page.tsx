// Server component for static export.
// generateStaticParams must be defined directly in the page file.
export function generateStaticParams() {
  return [{ projectId: "default" }];
}

// The AppShell is a client component. We import it via a thin client wrapper
// that receives projectId from the server component params.
import { AppShellClientWrapper } from "./client";

export default function Page({
  params,
}: {
  params: Promise<{ projectId: string }>;
}) {
  return <AppShellClientWrapper params={params} />;
}
