# CI Supply-Chain Maintenance

This project pins every external GitHub Action in `.github/workflows/*.yml` to a full 40-character commit SHA. The companion inventory at `.github/action-pins.jsonl` records the expected SHA and the human provenance note for each `(workflow, action)` pair.

The verifier is intentionally local and deterministic. It does not contact GitHub, create pull requests, push branches, or mutate files. It only compares workflow `uses:` entries against the checked-in inventory.

## Verifier

Agents should run the verifier through RCH:

```bash
rch exec -- ./scripts/verify-workflow-action-pins.sh
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

## Updating A Pin

When changing or adding an external action:

1. Resolve the desired upstream tag or commit yourself, for example with `git ls-remote --tags https://github.com/<owner>/<repo>.git refs/tags/<tag>`.
2. Update the workflow `uses:` entry to the exact 40-character SHA.
3. Update `.github/action-pins.jsonl` with the same workflow path, action name, SHA, tag/provenance label, and source note.
4. Run `rch exec -- ./scripts/verify-workflow-action-pins.sh`.
5. For workflow edits, also run `git diff --check`, `actionlint` if available, and the relevant targeted workflow harness.
6. Run `ubs` on the changed workflow, inventory, script, test, and docs files before committing.

This repository's integration branch is `main`. Any legacy branch mirroring is an explicit release/operator responsibility and should not be reintroduced as a workflow trigger target.
