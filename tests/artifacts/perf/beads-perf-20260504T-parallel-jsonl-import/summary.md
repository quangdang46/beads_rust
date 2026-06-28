# JSONL import comment-id collision probe optimization

Bead: `beads_rust-72yf.5`
Date: 2026-05-04

## Change

`sync_comments_for_import` now attempts the explicit comment-id insert first and only probes `comments(id)` after a real primary-key/unique collision. This preserves the existing behavior for cross-issue comment-id collisions while removing one SQLite lookup per imported comment on the common non-colliding import path.

## Workload

Source corpus:

- `/data/tmp/br-parallel-export-work-20260504T0628/.beads/issues.jsonl`
- 12,000 JSONL issue records
- SHA-256: `30eb851908afb1a054da8248c6e406d79d2cc8caabc135d4c61e1326a4a7cf8a`
- Each issue includes a stable synthetic comment payload

Fresh workspaces were initialized with the matching binary, then the corpus was copied into `.beads/issues.jsonl`. The timed command was `sync --import-only --force --json`.

## Paired Release Result

Baseline binary:

- Commit: `0ce1e990`
- Binary: `/data/tmp/br-baseline-comment-import-0ce1e990`
- Workspace: `/data/tmp/br-import-baseline-a-20260504-MTFADz`
- Result: `{"created":12000,"updated":0,"skipped":0,"tombstone_skipped":0,"orphans_removed":0,"blocked_cache_rebuilt":true}`
- Wall time: `5:07.18`
- User time: `305.82s`
- System time: `0.89s`
- Max RSS: `184948 KB`

Candidate binary:

- Commit base: `0ce1e990` plus this storage change
- Binary: `/data/tmp/br-candidate-comment-import-20260504`
- Workspace: `/data/tmp/br-import-candidate-a-20260504-F2ovXK`
- Result: `{"created":12000,"updated":0,"skipped":0,"tombstone_skipped":0,"orphans_removed":0,"blocked_cache_rebuilt":true}`
- Wall time: `4:53.11`
- User time: `291.83s`
- System time: `0.87s`
- Max RSS: `171480 KB`

Speedup:

- Wall time: `1.048x`
- User time: `1.048x`
- Max RSS reduction: `13468 KB`

## Verification

- `cargo fmt --check`
- `env CARGO_TARGET_DIR=/data/tmp/beads-rust-target-import-20260504 cargo test sync_comments_for_import --lib -- --nocapture`

