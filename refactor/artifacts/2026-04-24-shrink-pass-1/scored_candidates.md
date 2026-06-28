# Scored Candidates - 2026-04-24-shrink-pass-1

Acceptance threshold: `(LOC_saved * Confidence) / Risk >= 2.0`

| ID | Predicted LOC | LOC pts | Conf | Risk | Score | Rung | Decision |
|----|---------------|---------|------|------|-------|------|----------|
| D1 | -16 to -20 | 2 | 5 | 1 | 10.0 | 1 mechanical helper collapse | Accepted by score, blocked by red baseline. |
| D2 | Unknown, likely moderate | 3 | 3 | 3 | 3.0 | 2 row accessor helper extraction | Rejected for this pass because `src/storage/sqlite.rs` is actively reserved by another agent. |

## Execution order if gates become green

1. D1 first: lowest risk, test-only, no behavior change to production code.
2. Re-scan after D1 and only consider D2 if the storage reservation is released and the storage test baseline is green.

## Pass outcome

No candidate was executed. The initial baseline test suite was red, clippy was red, and the shared branch/worktree moved during the pass. Source edits would not have had a trustworthy isomorphism proof.
