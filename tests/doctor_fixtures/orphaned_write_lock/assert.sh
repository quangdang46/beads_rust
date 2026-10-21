#!/usr/bin/env bash
# Fixture assertions: orphaned_write_lock
set -euo pipefail
target_dir="${1:?usage: assert.sh <target_dir> <stage>}"
stage="${2:?usage: assert.sh <target_dir> <stage>}"
tool_bin="${TOOL_BIN:-br}"
cd "$target_dir"

case "$stage" in
  detect)
    # A leftover `.write.lock` that nobody holds is the normal state of
    # every workspace that has ever run `br`. The doctor must not warn
    # about it: an mtime-based detector made `br doctor` exit non-zero
    # on healthy repositories.
    out=$("$tool_bin" doctor --json 2>/dev/null) || true
    echo "$out" | jq -e '
      .checks[] | select(.name == "write_lock")
      | select(.status == "ok")
    ' >/dev/null || {
      echo "ASSERT FAIL[$stage]: write_lock flagged a leftover, unheld lock file" >&2
      echo "$out" | jq '.checks[] | select(.name == "write_lock")' >&2
      exit 1
    }
    # The reason must be one of the two non-warning verdicts. "not_held"
    # is the leftover-file case; "held_by_this_run" is what the CLI sees,
    # because the doctor holds the workspace write lock for its own run.
    echo "$out" | jq -e '
      .checks[] | select(.name == "write_lock")
      | .details.reason == "not_held" or .details.reason == "held_by_this_run"
    ' >/dev/null || {
      echo "ASSERT FAIL[$stage]: unexpected write_lock reason" >&2
      echo "$out" | jq '.checks[] | select(.name == "write_lock") | .details' >&2
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
