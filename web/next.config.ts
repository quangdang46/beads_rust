import type { NextConfig } from "next";
import { execSync } from "node:child_process";

// Build metadata baked in at build time (bead wxu). BUILD_NUMBER = git commit
// count; BUILD_SHA = 7-char short hash. CI can override via env vars of the same
// name; falls back to empty (the badge then hides) when git is unavailable.
function git(cmd: string): string {
  try {
    return execSync(cmd, { stdio: ["ignore", "pipe", "ignore"] }).toString().trim();
  } catch {
    return "";
  }
}
const BUILD_NUMBER = process.env.BUILD_NUMBER || git("git rev-list --count HEAD");
const BUILD_SHA = process.env.BUILD_SHA || git("git rev-parse --short=7 HEAD");

const nextConfig: NextConfig = {
  // Static export — the Rust server (src/web/) handles API routes.
  // Built only in CI via scripts/build-web.sh.
  ...(process.env.NEXT_EXPORT ? { output: "export" as const } : {}),
  distDir: "out",
  images: { unoptimized: true },
  typescript: { ignoreBuildErrors: true },
  eslint: { ignoreDuringBuilds: true },
  env: {
    NEXT_PUBLIC_BUILD_NUMBER: BUILD_NUMBER,
    NEXT_PUBLIC_BUILD_SHA: BUILD_SHA,
  },
};

export default nextConfig;
