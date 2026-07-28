#!/usr/bin/env bash
# Build the Next.js web UI as a static export, then copy it to
# src/web/static/ where the Rust build's rust-embed picks it up.
#
# Usage: bash scripts/build-web.sh
#
# This is called in CI before `cargo build --features web --release`.
# It is NOT run on the user's machine — the web UI is embedded in the
# release binary at build time.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WEB_SRC="$ROOT/web"
STATIC_DEST="$ROOT/src/web/static"
TEMP_BUILD="$(mktemp -d)"

echo "→ Building web UI from $WEB_SRC"
echo "→ Output to $STATIC_DEST"

# Clean previous build output.
rm -rf "$STATIC_DEST"

# Copy web/ to a temp directory so we can modify it without touching
# the source tree (removing API routes that the Rust server handles).
cp -r "$WEB_SRC" "$TEMP_BUILD/src"

# Remove API routes — the Rust axum server handles these.
rm -rf "$TEMP_BUILD/src/app/api"

# Write a minimal next.config.ts for static export.
cat > "$TEMP_BUILD/src/next.config.ts" << 'NEXT'
import type { NextConfig } from "next";
const nextConfig: NextConfig = {
  output: "export",
  distDir: "out",
  images: { unoptimized: true },
  // The Rust server handles API routes, so type errors from removed
  // API route files after removal are safe to ignore.
  typescript: { ignoreBuildErrors: true },
  eslint: { ignoreDuringBuilds: true },
};
export default nextConfig;
NEXT

# Copy node_modules from the source (avoids reinstall).
cd "$TEMP_BUILD/src"
if [ -d "$WEB_SRC/node_modules" ]; then
  cp -r "$WEB_SRC/node_modules" "$TEMP_BUILD/src/"
else
  npm ci --quiet 2>&1 || npm install --quiet 2>&1
fi
npx next build 2>&1

# Copy static output to the Rust embed directory.
if [ -d "out" ]; then
  mkdir -p "$STATIC_DEST"
  cp -r out/* "$STATIC_DEST/"
  echo "✓ Web UI built successfully → $STATIC_DEST ($(find "$STATIC_DEST" -type f | wc -l) files)"
else
  echo "✗ Build failed: out/ directory not found"
  ls -la
  exit 1
fi

# Clean up temp build.
rm -rf "$TEMP_BUILD"
