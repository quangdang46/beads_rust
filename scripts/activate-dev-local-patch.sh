#!/usr/bin/env bash
# Activate the sibling-path frankensqlite/fastmcp_rust patches for local
# co-development. Cargo.lock is marked skip-worktree so dev-local rewrites
# (stripping registry source/checksum lines from patched crates) are not
# accidentally staged or committed. Pair with `deactivate-dev-local-patch.sh`.
#
# Usage:
#   scripts/activate-dev-local-patch.sh           # frankensqlite only
#   scripts/activate-dev-local-patch.sh --fastmcp # also enable fastmcp_rust
#
# Idempotent: safe to re-run.
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

if [[ ! -f scripts/dev-local-frankensqlite.toml ]]; then
  echo "error: scripts/dev-local-frankensqlite.toml not found" >&2
  exit 1
fi

# `git update-index --skip-worktree` requires the file to be in the index.
# Bail early with an actionable message instead of letting that fail later.
if ! git ls-files --error-unmatch Cargo.lock >/dev/null 2>&1; then
  echo "error: Cargo.lock is not tracked in this branch" >&2
  echo "       this script depends on the Cargo.lock-tracking commit; rebase or" >&2
  echo "       update to a branch where Cargo.lock is committed before activating" >&2
  exit 1
fi

case "${1:-}" in
  "")
    ENABLE_FASTMCP=false
    ;;
  "--fastmcp")
    ENABLE_FASTMCP=true
    ;;
  *)
    echo "error: unknown flag '${1}'" >&2
    echo "       supported: (none) | --fastmcp" >&2
    exit 1
    ;;
esac

if [[ ! -d ../frankensqlite/crates/fsqlite ]]; then
  echo "error: sibling frankensqlite checkout not found at ../frankensqlite" >&2
  echo "       run \`git clone https://github.com/Dicklesworthstone/frankensqlite ../frankensqlite\` first" >&2
  exit 1
fi

if [[ "$ENABLE_FASTMCP" == true && ! -d ../fastmcp_rust/crates/fastmcp ]]; then
  echo "error: sibling fastmcp_rust checkout not found at ../fastmcp_rust" >&2
  echo "       run \`git clone https://github.com/Dicklesworthstone/fastmcp_rust ../fastmcp_rust\` first" >&2
  exit 1
fi

mkdir -p .cargo

# Refuse to clobber a `.cargo/config.toml` that wasn't planted by this script.
# The dev-local template starts with the literal marker comment below, so any
# config.toml that doesn't begin with that line is something the contributor
# wrote themselves and we leave it alone.
ACTIVATION_MARKER='# Local contributor template:'
if [[ -f .cargo/config.toml ]] \
   && ! head -1 .cargo/config.toml | grep -Fq "$ACTIVATION_MARKER"; then
  echo "error: .cargo/config.toml exists and was not activated by this script" >&2
  echo "       move it aside (e.g., \`mv .cargo/config.toml .cargo/config.toml.bak\`)" >&2
  echo "       before activating, or merge its contents into the dev-local template" >&2
  exit 1
fi

if [[ "$ENABLE_FASTMCP" == true ]]; then
  # Strip the leading `# ` from the fastmcp_rust block so those entries activate too.
  sed -E 's/^# (fastmcp-[a-z]+ +=)/\1/' scripts/dev-local-frankensqlite.toml > .cargo/config.toml
  echo "activated dev-local patch (frankensqlite + fastmcp_rust)"
else
  cp scripts/dev-local-frankensqlite.toml .cargo/config.toml
  echo "activated dev-local patch (frankensqlite only — pass --fastmcp to also enable fastmcp_rust)"
fi

git update-index --skip-worktree Cargo.lock
echo "Cargo.lock: skip-worktree on (cargo rewrites are now invisible to git)"
