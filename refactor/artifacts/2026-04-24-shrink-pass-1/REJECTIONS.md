# Rejections - 2026-04-24-shrink-pass-1

| ID | Why rejected or deferred | Action taken |
|----|---------------------------|--------------|
| D1 | Accepted by score, but deferred because baseline tests and clippy were red before source edits. | Wrote card only. No source edit. |
| D2 | `src/storage/sqlite.rs` is actively reserved by `JadeCondor`; touching it would collide with peer work. | Did not prepare an edit. Kept the candidate in the duplication map for later re-scan. |
| G1 | Broad repository simplification pass. | Rejected for this pass because the repo has active peer edits and a broken baseline; broad refactors would make attribution and proof worse. |
