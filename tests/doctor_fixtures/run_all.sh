#!/usr/bin/env bash
# Phase 9: real-world fixture suite driver for `br doctor`.
#
# Iterates every subdirectory under this script's location, plants the
# failure via that dir's corrupt.sh, runs br doctor / br doctor --repair /
# br doctor undo latest, and checks each stage's assert.sh.
#
# Exit 0 if every fixture passes; non-zero (1) on the first failure with a
# clear diagnostic. Per-fixture isolation is provided by tempdir; the
# source tree is never mutated.
#
# Env:
#   TOOL_BIN  — path to the `br` binary (default: $CARGO_BIN_EXE_br, or
#               `cargo run --quiet --bin br --`)
#   FIXTURES_ROOT — override the fixtures directory
#   SKIP — space-separated fixture names to skip
#   ONLY — space-separated allowlist of fixture names; everything else skipped
#   FAIL_FAST — if "1" (default), exit on first failure; if "0", run all
#   REPLAY_IDEMPOTENCE — if "1", run --repair a second time and require
#                        newly-created replay run actions to be empty
#   REPLAY_IDEMPOTENCE_SKIP — space-separated fixture names to skip for replay

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIXTURES_ROOT="${FIXTURES_ROOT:-$SCRIPT_DIR}"
FAIL_FAST="${FAIL_FAST:-1}"

if [ -z "${TOOL_BIN:-}" ]; then
    if [ -n "${CARGO_BIN_EXE_br:-}" ]; then
        TOOL_BIN="$CARGO_BIN_EXE_br"
    elif command -v br >/dev/null 2>&1; then
        TOOL_BIN="$(command -v br)"
    else
        echo "run_all.sh: cannot locate \`br\` binary (set TOOL_BIN or CARGO_BIN_EXE_br)" >&2
        exit 2
    fi
fi
export TOOL_BIN

if [ ! -x "$TOOL_BIN" ]; then
    echo "run_all.sh: TOOL_BIN=$TOOL_BIN is not executable" >&2
    exit 2
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "run_all.sh: \`jq\` is required (apt-get install jq)" >&2
    exit 2
fi

declare -a fixtures=()
while IFS= read -r dir; do
    fixtures+=("$dir")
done < <(find "$FIXTURES_ROOT" -mindepth 1 -maxdepth 1 -type d | sort)

if [ "${#fixtures[@]}" -eq 0 ]; then
    echo "run_all.sh: no fixtures found under $FIXTURES_ROOT" >&2
    exit 2
fi

total=${#fixtures[@]}
pass=0
fail=0
skipped=0

contains() {
    local needle="$1"; shift
    for item in "$@"; do [ "$item" = "$needle" ] && return 0; done
    return 1
}

list_run_ids() {
    local runs_dir="$1"
    [ -d "$runs_dir" ] || return 0
    find "$runs_dir" -maxdepth 1 -mindepth 1 -type d -exec basename {} \; | sort
}

# Parse allowlist/blocklist
ONLY_LIST=()
SKIP_LIST=()
if [ -n "${ONLY:-}" ]; then
    read -ra ONLY_LIST <<<"$ONLY"
fi
if [ -n "${SKIP:-}" ]; then
    read -ra SKIP_LIST <<<"$SKIP"
fi

# Returns 0 on pass, 1 on fail, 2 on skip.
run_fixture() {
    local fixture_dir="$1"
    local name; name="$(basename "$fixture_dir")"

    if [ "${#ONLY_LIST[@]}" -gt 0 ] && ! contains "$name" "${ONLY_LIST[@]}"; then
        return 2
    fi
    if [ "${#SKIP_LIST[@]}" -gt 0 ] && contains "$name" "${SKIP_LIST[@]}"; then
        echo "[SKIP] $name (explicitly listed in SKIP)"
        return 2
    fi

    local corrupt_sh="$fixture_dir/corrupt.sh"
    local assert_sh="$fixture_dir/assert.sh"
    if [ ! -x "$corrupt_sh" ] || [ ! -x "$assert_sh" ]; then
        echo "[FAIL] $name: corrupt.sh or assert.sh missing/non-executable" >&2
        return 1
    fi

    local tmp; tmp="$(mktemp -d -t br-doctor-fixture-XXXXXX)"
    local diag="$tmp/_diag"
    mkdir -p "$diag"

    # `br doctor` walks parent directories to discover `.beads/`. We pin it
    # to the fixture tempdir by:
    #   1. cd-ing into $tmp before invoking br/scripts
    #   2. exporting BEADS_DIR=$tmp/.beads so even nested invocations honor it
    #   3. clearing BD_*, BEADS_*, RUST_LOG-noisy env that the developer's
    #      shell may have set
    # We do NOT use `env -i` because it strips PATH and PWD, and br needs PATH
    # to discover `git` for fixture-side `git init`.
    local doctor_env=(
        env
        HOME="$tmp"
        NO_COLOR=1
        RUST_LOG=error
        TOOL_BIN="$TOOL_BIN"
        BR_NO_AUTOFLUSH=1
        BEADS_DIR="$tmp/.beads"
        # Strip developer-shell beads overrides.
        --unset=BD_DB --unset=BD_DATABASE --unset=BEADS_DB
        --unset=BR_STARTUP_CACHE
    )

    # Stage 1: plant the failure.
    if ! ( cd "$tmp" && "${doctor_env[@]}" bash "$corrupt_sh" "$tmp" ) \
            > "$diag/corrupt.stdout" 2> "$diag/corrupt.stderr"; then
        echo "[FAIL] $name: corrupt stage failed" >&2
        sed 's/^/  /' "$diag/corrupt.stderr" >&2
        echo "  (workspace at $tmp)" >&2
        return 1
    fi

    # Stage 2: detect-stage assertions.
    if ! ( cd "$tmp" && "${doctor_env[@]}" bash "$assert_sh" "$tmp" detect ) \
            > "$diag/detect.stdout" 2> "$diag/detect.stderr"; then
        echo "[FAIL] $name: detect stage failed" >&2
        sed 's/^/  /' "$diag/detect.stderr" >&2
        echo "  (workspace at $tmp)" >&2
        return 1
    fi

    # Stage 3: --repair (don't abort on non-zero exit — assert.sh judges).
    ( cd "$tmp" && "${doctor_env[@]}" "$TOOL_BIN" doctor --repair --json ) \
        > "$diag/repair.json" 2> "$diag/repair.stderr" || true

    # Stage 3.5 (pass-3, opt-in): idempotence replay gate. The
    # chokepoint contract requires that running `--repair` twice
    # in a row is a no-op on the second invocation. A second run
    # that produces any non-empty actions.jsonl line means either
    # the detector is impure (mutates a side-channel) or the fixer
    # isn't idempotent.
    #
    # OPT-IN: REPLAY_IDEMPOTENCE=1 enables the gate. Default off
    # because the default suite's post_undo stages assert that
    # `undo latest` reverses the FIRST repair; a second --repair
    # creates a no-op run-dir that becomes the new "latest",
    # which would break post_undo for fixtures whose corruption
    # the repair successfully clears (e.g., gitignore fixers).
    #
    # CI / pass-3 idempotence-audit invocation:
    #   REPLAY_IDEMPOTENCE=1 \
    #   REPLAY_IDEMPOTENCE_SKIP="gitignore_leaking_beads gitignore_bare_pattern" \
    #   bash tests/doctor_fixtures/run_all.sh
    #
    # Per-fixture opt-out (independent of the suite-level gate):
    # drop a `.skip_replay` marker file inside the fixture dir.
    if [ "${REPLAY_IDEMPOTENCE:-0}" = "1" ]; then
        local skip_replay=0
        if [ -n "${REPLAY_IDEMPOTENCE_SKIP:-}" ]; then
            local skip_item
            for skip_item in ${REPLAY_IDEMPOTENCE_SKIP}; do
                if [ "$skip_item" = "$name" ]; then
                    skip_replay=1
                    break
                fi
            done
        fi
        if [ -f "$fixture_dir/.skip_replay" ]; then
            skip_replay=1
        fi
        if [ "$skip_replay" -eq 0 ]; then
            local runs_dir="$tmp/.doctor/runs"
            local before_runs="$diag/replay_runs.before"
            local after_runs="$diag/replay_runs.after"
            list_run_ids "$runs_dir" > "$before_runs"
            ( cd "$tmp" && "${doctor_env[@]}" "$TOOL_BIN" doctor --repair --json ) \
                > "$diag/repair_replay.json" 2> "$diag/repair_replay.stderr" || true
            list_run_ids "$runs_dir" > "$after_runs"
            local new_run_ids=()
            mapfile -t new_run_ids < <(comm -13 "$before_runs" "$after_runs")
            local new_run_id
            for new_run_id in "${new_run_ids[@]}"; do
                local newest_run="$runs_dir/$new_run_id"
                if [ -f "$newest_run/actions.jsonl" ]; then
                    local replay_action_count
                    replay_action_count="$(grep -c -v '^[[:space:]]*$' "$newest_run/actions.jsonl" 2>/dev/null || echo 0)"
                    replay_action_count="${replay_action_count//[[:space:]]/}"
                    if [ "${replay_action_count:-0}" -gt 0 ]; then
                        echo "[FAIL] $name: idempotence replay failed — second --repair produced $replay_action_count action(s)" >&2
                        echo "  --- replay actions.jsonl ---" >&2
                        sed 's/^/  /' "$newest_run/actions.jsonl" >&2
                        echo "  (workspace at $tmp)" >&2
                        return 1
                    fi
                fi
            done
        fi
    fi

    # Stage 4: post_repair assertions.
    if ! ( cd "$tmp" && "${doctor_env[@]}" bash "$assert_sh" "$tmp" post_repair ) \
            > "$diag/post_repair.stdout" 2> "$diag/post_repair.stderr"; then
        echo "[FAIL] $name: post_repair stage failed" >&2
        sed 's/^/  /' "$diag/post_repair.stderr" >&2
        echo "  --- repair.json head ---" >&2
        head -c 1024 "$diag/repair.json" >&2 || true
        echo >&2
        echo "  (workspace at $tmp)" >&2
        return 1
    fi

    # Stage 5: undo latest (best-effort).
    ( cd "$tmp" && "${doctor_env[@]}" "$TOOL_BIN" doctor undo latest --json ) \
        > "$diag/undo.json" 2> "$diag/undo.stderr" || true

    # Stage 6: post_undo assertions.
    if ! ( cd "$tmp" && "${doctor_env[@]}" bash "$assert_sh" "$tmp" post_undo ) \
            > "$diag/post_undo.stdout" 2> "$diag/post_undo.stderr"; then
        echo "[FAIL] $name: post_undo stage failed" >&2
        sed 's/^/  /' "$diag/post_undo.stderr" >&2
        echo "  --- undo.json ---" >&2
        cat "$diag/undo.json" >&2 || true
        echo "  (workspace at $tmp)" >&2
        return 1
    fi

    echo "[PASS] $name (workspace retained: $tmp)"
    return 0
}

echo "run_all.sh: $total fixture(s) under $FIXTURES_ROOT"
echo "run_all.sh: TOOL_BIN=$TOOL_BIN"
echo

for fixture_dir in "${fixtures[@]}"; do
    rc=0
    run_fixture "$fixture_dir" || rc=$?
    case "$rc" in
        0) pass=$((pass+1)) ;;
        1)
            fail=$((fail+1))
            if [ "$FAIL_FAST" = "1" ]; then
                echo
                echo "Summary: pass=$pass fail=$fail skipped=$skipped of $total"
                exit 1
            fi
            ;;
        2) skipped=$((skipped+1)) ;;
        *) fail=$((fail+1)) ;;
    esac
done

echo
echo "Summary: pass=$pass fail=$fail skipped=$skipped of $total"
[ "$fail" -eq 0 ] || exit 1
