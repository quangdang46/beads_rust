#!/usr/bin/env bash
# Fixture: recovery_artifacts_aged
# FM: fm-state_files-recovery-artifacts-orphaned (P3) — TTL-quarantine variant
#
# Plants two recovery artifacts under .beads/.br_recovery/:
#   - one aged (mtime > RECOVERY_AGED_TTL_DAYS) — should be quarantined
#   - one recent (mtime < TTL) — should be PRESERVED in place
# Also plants one aged .bad_<TS> sibling next to the live DB to exercise
# the second branch of recovery_artifacts_for_db_family.

set -euo pipefail
target_dir="${1:?usage: corrupt.sh <target_dir>}"
tool_bin="${TOOL_BIN:-br}"

mkdir -p "$target_dir"
cd "$target_dir"
"$tool_bin" init >/dev/null 2>&1

recovery="$target_dir/.beads/.br_recovery"
mkdir -p "$recovery"

# Aged: 60 days old (well past the 30-day TTL).
aged_path="$recovery/beads.db.20250101T000000Z"
cp .beads/beads.db "$aged_path"
touch -d '60 days ago' "$aged_path"

# Recent: 1 day old (well within the TTL).
recent_path="$recovery/beads.db.20260512T000000Z"
cp .beads/beads.db "$recent_path"
touch -d '1 day ago' "$recent_path"

# Aged sibling .bad_<TS> next to the live DB.
bad_sibling="$target_dir/.beads/beads.db.bad_20250101T000000Z"
cp .beads/beads.db "$bad_sibling"
touch -d '60 days ago' "$bad_sibling"

# Record the paths for assert.sh.
{
  echo "AGED=$aged_path"
  echo "RECENT=$recent_path"
  echo "BAD_SIBLING=$bad_sibling"
} > .fixture_paths

if [ -e .fixture_baseline ]; then
  echo "fixture baseline already exists; expected a fresh workspace" >&2
  exit 1
fi
mkdir -p .fixture_baseline
tar --exclude=.fixture_baseline -cf .fixture_baseline/state.tar .
