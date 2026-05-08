# CI Supply-Chain Maintenance

This project pins every external GitHub Action in `.github/workflows/*.yml` to a full 40-character commit SHA. The companion inventory at `.github/action-pins.jsonl` records the expected SHA and the human provenance note for each `(workflow, action)` pair.

The verifier is intentionally local and deterministic. It does not contact GitHub, create pull requests, push branches, or mutate files. It only compares workflow `uses:` entries against the checked-in inventory.

## Verifier

Agents should run the verifier's Cargo target through RCH:

```bash
rch exec -- env CARGO_TARGET_DIR=${TMPDIR:-/tmp}/rch_target_beads_rust_ci_supply cargo test --test workflow_action_pins -- --nocapture
```

Local operators can run the same script directly:

```bash
./scripts/verify-workflow-action-pins.sh
```

The script runs `cargo test --test workflow_action_pins -- --nocapture`. The test fails when:

- a workflow uses an external action without an `@` reference,
- a workflow uses a tag, branch, or short SHA instead of a 40-character SHA,
- a pinned action is missing from `.github/action-pins.jsonl`,
- the inventory SHA disagrees with the workflow SHA,
- the inventory has malformed or stale entries.

Local actions such as `./path/to/action` are ignored by this verifier.

## Update Audit

The update audit is report-only. It reads `.github/action-pins.jsonl` and the configured upstream policy in `.github/action-pin-upstreams.jsonl`, then reports whether each checked-in action pin is up to date, has an allowed update available, points beyond the configured allowed tag, or needs upstream investigation.

```bash
./scripts/audit-workflow-action-pins.sh --format json
./scripts/audit-workflow-action-pins.sh --format text
```

The JSON report includes `action`, `current_tag`, `current_sha`, `latest_allowed_tag`, `latest_allowed_sha`, `status`, and `manual_update_steps` for every inventory row. The script does not edit workflows, rewrite the inventory, create pull requests, push branches, or contact GitHub by default.

Live upstream resolution is explicit:

```bash
./scripts/audit-workflow-action-pins.sh --live --timeout 10 --format json
```

Live mode runs bounded `git ls-remote` lookups for the configured upstream refs and reports `upstream_unreachable` or `missing_tag` instead of mutating files.

Release workflow shell fragments have a separate focused harness:

```bash
./scripts/verify-release-workflow-fragments.sh
```

Agents should run the same test target through RCH:

```bash
rch exec -- env CARGO_TARGET_DIR=${TMPDIR:-/tmp}/rch_target_beads_rust_ci_supply cargo test --test workflow_release_fragments -- --nocapture
```

That harness parses `.github/workflows/release.yml` and executes the high-risk release fragments against fixtures for reliability override validation, required artifact detection, checksum aggregation, checksum verification, and release-note branch coverage.

The ACFS installer notification workflow also has a focused local harness:

```bash
./scripts/verify-notify-acfs-workflow.sh
```

Agents should run the same test target through RCH:

```bash
rch exec -- env CARGO_TARGET_DIR=${TMPDIR:-/tmp}/rch_target_beads_rust_ci_supply cargo test --test workflow_notify_acfs -- --nocapture
```

That harness parses `.github/workflows/notify-acfs.yml` and checks the installer checksum, previous-checksum fallback, changed/unchanged comparison, dry-run branch, missing-token notice, repository-dispatch payload, `main` branch trigger, and summary output without sending network notifications.

## Updating A Pin

When changing or adding an external action:

1. Resolve the desired upstream tag or commit yourself, for example with `git ls-remote --tags https://github.com/<owner>/<repo>.git refs/tags/<tag>`.
2. Update `.github/action-pin-upstreams.jsonl` with the reviewed `latest_allowed_tag`, `latest_allowed_sha`, and source note.
3. Run `./scripts/audit-workflow-action-pins.sh --format json` and review the `manual_update_steps` for each affected row.
4. Update the workflow `uses:` entry to the exact 40-character SHA.
5. Update `.github/action-pins.jsonl` with the same workflow path, action name, SHA, tag/provenance label, and source note.
6. Run `rch exec -- env CARGO_TARGET_DIR=${TMPDIR:-/tmp}/rch_target_beads_rust_ci_supply cargo test --test workflow_action_pins -- --nocapture`.
7. For workflow edits, also run `git diff --check`, `actionlint` if available, and the relevant targeted workflow harness such as `./scripts/verify-release-workflow-fragments.sh` or `./scripts/verify-notify-acfs-workflow.sh`.
8. Run `ubs` on the changed workflow, inventory, script, test, and docs files before committing.

This repository's integration branch is `main`. Any legacy branch mirroring is an explicit release/operator responsibility and should not be reintroduced as a workflow trigger target.
