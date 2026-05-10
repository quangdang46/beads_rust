#!/usr/bin/env bash
# Fixture assertions: merge_artifact_stuck
set -euo pipefail
target_dir="${1:?usage: assert.sh <target_dir> <stage>}"
stage="${2:?usage: assert.sh <target_dir> <stage>}"
tool_bin="${TOOL_BIN:-br}"
cd "$target_dir"

case "$stage" in
  detect)
    out=$("$tool_bin" doctor --json 2>/dev/null) || true
    echo "$out" | jq -e '
      .checks[] | select(.name == "jsonl.merge_artifacts")
      | select(.status == "warn" or .status == "error")
    ' >/dev/null || {
      echo "ASSERT FAIL[$stage]: jsonl.merge_artifacts not flagged" >&2
      echo "$out" | jq '.checks[] | select(.name == "jsonl.merge_artifacts")' >&2
      exit 1
    }
    [ -f .beads/issues.base.jsonl ] || { echo "ASSERT FAIL[$stage]: base.jsonl missing" >&2; exit 1; }
    ;;
  post_repair)
    # Detect-only FM. Artifacts should remain (no destructive auto-delete).
    [ -f .beads/issues.base.jsonl ] || {
      echo "ASSERT FAIL[$stage]: issues.base.jsonl was auto-deleted by --repair (unsafe)" >&2
      exit 1
    }
    ;;
  post_undo)
    [ -d .beads ] || { echo "ASSERT FAIL[$stage]: .beads gone after undo" >&2; exit 1; }
    ;;
  *)
    echo "unknown stage: $stage" >&2
    exit 2
    ;;
esac
