#!/usr/bin/env bash
# Fixture: recovery_dir_not_writable
# FM: fm-permissions-recovery-dir-not-writable (P2)
#
# Initialises a workspace, creates .beads/.br_recovery/, plants a
# seed marker file, then chmods the dir to 0o555 (owner-write
# stripped). Doctor's `permissions.recovery_dir` check should fire
# warn; `--repair` must NOT silently chmod the directory.

set -euo pipefail
target_dir="${1:?usage: corrupt.sh <target_dir>}"
tool_bin="${TOOL_BIN:-br}"

mkdir -p "$target_dir"
cd "$target_dir"
"$tool_bin" init >/dev/null 2>&1
"$tool_bin" create --title "fixture issue 1" --type task --priority 2 >/dev/null 2>&1
"$tool_bin" sync --flush-only >/dev/null 2>&1

mkdir -p .beads/.br_recovery
echo "recovery-fixture-seed" > .beads/.br_recovery/.fixture_seed_marker
chmod 0555 .beads/.br_recovery

# Snapshot the corrupted mode for diagnostics without relying on
# GNU-only stat flags.
python3 - .beads/.br_recovery > .fixture_pre_mode <<'PY'
import os
import sys

print(format(os.stat(sys.argv[1]).st_mode & 0o777, "o"))
PY

if [ -e .fixture_baseline ]; then
  echo "fixture baseline already exists; expected a fresh workspace" >&2
  exit 1
fi
mkdir -p .fixture_baseline
tar --exclude=.fixture_baseline -cf .fixture_baseline/state.tar .
