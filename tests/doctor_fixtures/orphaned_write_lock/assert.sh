#!/usr/bin/env bash
# Fixture assertions: orphaned_write_lock
set -euo pipefail
target_dir="${1:?usage: assert.sh <target_dir> <stage>}"
stage="${2:?usage: assert.sh <target_dir> <stage>}"
tool_bin="${TOOL_BIN:-br}"
cd "$target_dir"

# Force the stale branch by overriding the staleness threshold to 0.
# Any non-future mtime is then "older than threshold" → warn.
export BR_DOCTOR_STALE_LOCK_THRESHOLD_SECS=0

case "$stage" in
  detect)
    out=$("$tool_bin" doctor --json 2>/dev/null) || true
    echo "$out" | jq -e '
      .checks[] | select(.name == "write_lock")
      | select(.status == "warn" or .status == "error")
    ' >/dev/null || {
      echo "ASSERT FAIL[$stage]: write_lock not flagged" >&2
      echo "$out" | jq '.checks[] | select(.name == "write_lock")' >&2
      exit 1
    }
    # reason must be stale_mtime.
    echo "$out" | jq -e '
      .checks[] | select(.name == "write_lock")
      | .details.reason == "stale_mtime"
    ' >/dev/null || {
      echo "ASSERT FAIL[$stage]: details.reason != stale_mtime" >&2
      echo "$out" | jq '.checks[] | select(.name == "write_lock") | .details' >&2
      exit 1
    }
    # recommended_fix must include the .stale-<ts> rename suffix.
    echo "$out" | jq -e '
      .checks[] | select(.name == "write_lock")
      | .details.recommended_fix | test("\\.stale-")
    ' >/dev/null || {
      echo "ASSERT FAIL[$stage]: recommended_fix missing .stale- rename suffix" >&2
      exit 1
    }
    ;;
  post_repair)
    # Detect-only: .write.lock MUST still be present byte-identically
    # (doctor must NEVER auto-remove). The fact that --repair completed
    # without mutating it is the safety guarantee.
    [ -f .beads/.write.lock ] || {
      echo "ASSERT FAIL[$stage]: .write.lock vanished after --repair (unsafe; could corrupt a live writer)" >&2
      exit 1
    }
    if [ -L .beads/.write.lock ]; then
      echo "ASSERT FAIL[$stage]: .write.lock became a symlink after --repair (unsafe)" >&2
      exit 1
    fi
    ;;
  post_undo)
    [ -d .beads ] || { echo "ASSERT FAIL[$stage]: .beads gone after undo" >&2; exit 1; }
    [ -f .beads/.write.lock ] || { echo "ASSERT FAIL[$stage]: .write.lock gone after undo" >&2; exit 1; }
    ;;
  *)
    echo "unknown stage: $stage" >&2
    exit 2
    ;;
esac
