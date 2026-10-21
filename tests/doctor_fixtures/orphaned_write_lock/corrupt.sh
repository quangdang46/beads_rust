#!/usr/bin/env bash
# Fixture: orphaned_write_lock
# FM: fm-concurrency_primitives-orphaned-write-lock (P1) — detect-only.
#
# Plants a regular-file `.beads/.write.lock` that no process holds —
# exactly the state every workspace is left in after any `br` command,
# because the advisory flock is released on exit and the file is never
# unlinked. The doctor must report it as healthy.
#
# The "lock is held by a live process" branch needs a live holder, which
# bash cannot create portably; it is covered by the Rust unit test
# `check_orphaned_write_lock_held_is_warn`.

set -euo pipefail
target_dir="${1:?usage: corrupt.sh <target_dir>}"
tool_bin="${TOOL_BIN:-br}"

mkdir -p "$target_dir"
cd "$target_dir"
"$tool_bin" init >/dev/null 2>&1

# Plant a regular-file write.lock. `touch` keeps the file empty.
: > .beads/.write.lock

if [ -e .fixture_baseline ]; then
  echo "fixture baseline already exists; expected a fresh workspace" >&2
  exit 1
fi
mkdir -p .fixture_baseline
tar --exclude=.fixture_baseline -cf .fixture_baseline/state.tar .
