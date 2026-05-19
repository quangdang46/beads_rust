#!/usr/bin/env bash
# Fixture assertions: recovery_dir_not_writable
#
# Pass-5 cycle 30: fm-permissions-recovery-dir-not-writable surfaces
# a read-only .beads/.br_recovery/ before --repair fails mid-backup.
# DETECT-ONLY by design — operators may intentionally lock it.

set -euo pipefail
target_dir="${1:?usage: assert.sh <target_dir> <stage>}"
stage="${2:?usage: assert.sh <target_dir> <stage>}"
tool_bin="${TOOL_BIN:-br}"
cd "$target_dir"

cur_mode() {
  python3 - .beads/.br_recovery <<'PY'
import os
import sys

print(format(os.stat(sys.argv[1]).st_mode & 0o777, "o"))
PY
}

case "$stage" in
  detect)
    out=$("$tool_bin" doctor --json 2>/dev/null) || true
    echo "$out" | jq -e '
      .checks[] | select(.name == "permissions.recovery_dir")
      | select(.status == "warn" or .status == "error")
    ' >/dev/null || {
      echo "ASSERT FAIL[$stage]: permissions.recovery_dir not flagged" >&2
      echo "$out" | jq '.checks[] | select(.name == "permissions.recovery_dir")' >&2
      exit 1
    }
    mode=$(cur_mode)
    if [ "$mode" != "555" ]; then
      echo "ASSERT FAIL[$stage]: expected pre-fix mode 555, got '$mode'" >&2
      exit 1
    fi
    echo "$out" | jq -e '
      .checks[] | select(.name == "permissions.recovery_dir")
      | .details.mode_octal == "555"
    ' >/dev/null || {
      echo "ASSERT FAIL[$stage]: permissions.recovery_dir details missing mode_octal=\"555\"" >&2
      echo "$out" | jq '.checks[] | select(.name == "permissions.recovery_dir") | .details' >&2
      exit 1
    }
    ;;
  post_repair)
    # SACRED INVARIANT: doctor must NOT silently chmod the dir.
    mode=$(cur_mode)
    if [ "$mode" != "555" ]; then
      echo "ASSERT FAIL[$stage]: doctor silently chmod'd .beads/.br_recovery from 0555 to 0$mode (operator intent stomped)" >&2
      exit 1
    fi
    # Restore so undo + TempDir cleanup can proceed.
    chmod 0755 .beads/.br_recovery
    ;;
  post_undo)
    [ -d .beads/.br_recovery ] || { echo "ASSERT FAIL[$stage]: .beads/.br_recovery gone after undo" >&2; exit 1; }
    if [ ! -f .beads/.br_recovery/.fixture_seed_marker ]; then
      echo "ASSERT FAIL[$stage]: seed marker missing after undo" >&2
      exit 1
    fi
    contents=$(cat .beads/.br_recovery/.fixture_seed_marker)
    if [ "$contents" != "recovery-fixture-seed" ]; then
      echo "ASSERT FAIL[$stage]: seed marker content drifted: '$contents'" >&2
      exit 1
    fi
    ;;
  *)
    echo "unknown stage: $stage" >&2
    exit 2
    ;;
esac
