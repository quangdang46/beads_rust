import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Static export — the Rust server (src/web/) handles API routes.
  // Built only in CI via scripts/build-web.sh.
  output: "export",
  distDir: "out",
  images: { unoptimized: true },
};

export default nextConfig;
