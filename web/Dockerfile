# ── br: download prebuilt beads_rust CLI from GitHub releases ──────────────
# br ships one libc-agnostic musl binary per Linux arch (br-linux-x64 /
# br-linux-arm64), so a plain tarball is all this stage needs.
# Docker's TARGETARCH is amd64/arm64; the release assets are x64/arm64.
#
# BR_VERSION is the one thing here that cannot be derived from Cargo.toml: the
# build context is web/ — `COPY package.json` and `COPY . .` both resolve
# inside it — so the manifest is not in the image to read. It tracks
# Cargo.toml by hand, and nothing here can catch it when it drifts: the
# version check below only proves the tarball's own binary reports the tag it
# was fetched under, so it catches a corrupt or mislabelled download and
# nothing else. A pin that resolves but trails the manifest passes silently.
# Two things must therefore both be true: the tag exists as a published
# release, and the tree's Cargo.toml version matches it. Keep them in step by
# cutting the release, not by editing this line.
#
# KNOWN BAD: 0.2.0 is the manifest version but was never published as a GitHub
# release (the old v0.2.0 tag was stale and has been reclaimed), so the
# download 404s and this image does not build. Last published is v0.1.3.
# Publishing v0.2.0 is what fixes this; pinning to v0.1.3 would build, but that
# binary predates the frankensqlite→rusqlite swap and would not read a current
# database.
ARG BR_VERSION=0.2.0
ARG NODE_VERSION=26.4.0
FROM alpine:3.22 AS br
ARG BR_VERSION
ARG TARGETARCH
RUN case "$TARGETARCH" in \
      amd64) asset_arch=x64 ;; \
      arm64) asset_arch=arm64 ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac \
 && archive="br-linux-${asset_arch}.tar.gz" \
 && tag="v${BR_VERSION#v}" \
 && wget -q -O "/tmp/${archive}" \
      "https://github.com/quangdang46/beads_rust/releases/download/${tag}/${archive}" \
 && tar -xzf "/tmp/${archive}" -C /usr/local/bin br \
 && actual="$(/usr/local/bin/br --version)" \
 && if [ "$actual" != "br ${tag#v}" ]; then \
      echo "tag ${tag} ships '${actual}', expected 'br ${tag#v}'" >&2; exit 1; \
    fi

# ── Eleventy: self-contained tree for the showcase publisher ─────────────────
# The app locates node_modules/@11ty/eleventy/cmd.cjs by scanning the
# filesystem (lib/showcase/generate.ts) — it is deliberately never imported, so
# Next's standalone output tracing does not include it. Install it with its
# full dependency tree here and copy it into the runner below.
# Keep the version in sync with package.json.
FROM node:${NODE_VERSION}-alpine AS eleventy
WORKDIR /eleventy
RUN npm install --no-save @11ty/eleventy@3.1.6

# ── Builder: install deps and build the Next.js app ──────────────────────────
FROM node:${NODE_VERSION}-alpine AS builder

WORKDIR /app

COPY package.json package-lock.json ./
RUN npm ci

COPY . .

# next.config.ts reads git for build metadata; git is unavailable in the
# container, but it falls back gracefully (badge hides). Override via build args
# if you want the badge to show a specific build/sha.
ARG BUILD_NUMBER=""
ARG BUILD_SHA=""

# Standalone output is opt-in (see next.config.ts) so local `next start` flows
# keep the default output.
ENV NEXT_STANDALONE=1

RUN npm run build

# ── Runner: minimal image with standalone output + br ───────────────────────
# Debian slim (not Alpine) is kept for the Node runtime, not for br: the br
# release binaries are musl-linked and static, so they run here as-is.
FROM node:${NODE_VERSION}-slim AS runner

WORKDIR /app

ENV NODE_ENV=production
ENV PORT=3000
ENV HOSTNAME=0.0.0.0
ENV HOME=/home/nextjs
ENV XDG_CONFIG_HOME=/home/nextjs/.config

# Run as a non-root user for security and give br a real home/config dir.
# The home/config dirs are world-writable so `docker run --user <uid>:<gid>`
# (the README's fix for mounted-volume permission errors) can still write app
# settings and br state — Debian's HOME_MODE 0700 would otherwise block any
# uid other than 1001 from even traversing /home/nextjs.
RUN groupadd --system --gid 1001 nodejs \
 && useradd --system --uid 1001 --gid nodejs --create-home --home-dir /home/nextjs nextjs \
 && mkdir -p /home/nextjs/.config \
 && chown -R nextjs:nodejs /home/nextjs \
 && chmod 0777 /home/nextjs /home/nextjs/.config

# Copy the standalone server, static assets, and public files.
# Next's file tracing puts a PARTIAL @11ty/eleventy (cmd.cjs without its deps)
# into the standalone node_modules; it would shadow the complete /node_modules
# tree below (eleventyBin() checks cwd first), so drop it.
COPY --from=builder --chown=nextjs:nodejs /app/.next/standalone ./
RUN rm -rf ./node_modules/@11ty
COPY --from=builder --chown=nextjs:nodejs /app/.next/static ./.next/static
COPY --from=builder --chown=nextjs:nodejs /app/public ./public

# Copy the prebuilt br CLI into the image
COPY --from=br --chown=nextjs:nodejs /usr/local/bin/br /usr/local/bin/br

# Eleventy for the showcase publisher. /node_modules sits on eleventyBin()'s
# upward search path and never collides with the standalone node_modules.
COPY --from=eleventy --chown=nextjs:nodejs /eleventy/node_modules /node_modules

USER nextjs

EXPOSE 3000

CMD ["node", "server.js"]
