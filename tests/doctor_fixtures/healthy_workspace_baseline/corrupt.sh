#!/usr/bin/env bash
# Fixture: healthy_workspace_baseline (control)
#
# Plants a clean `br init` workspace with no failure. Asserts the doctor
# health surface returns ok/degraded (per the "expected for frankensqlite"
# WAL-without-SHM warning) and that `--repair` is a no-op (idempotence).

set -euo pipefail
target_dir="${1:?usage: corrupt.sh <target_dir>}"
tool_bin="${TOOL_BIN:-br}"

mkdir -p "$target_dir"
cd "$target_dir"
"$tool_bin" init >/dev/null 2>&1

rm -rf .fixture_baseline
mkdir -p .fixture_baseline
tar --exclude=.fixture_baseline -cf .fixture_baseline/state.tar .
