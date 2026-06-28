# list text limit projection

Workload: `/tmp/br-blocked-projection-real2-tTf4Nx` with 12,000 issues.

Candidate: allow `list_text_issues_for_command_output` to keep using the compact
text/table projection when `--limit` or `--offset` is present, appending the same
SQL `LIMIT/OFFSET` clauses used by the full list query.

Behavior proof:

- `list --limit 20 --format text`: byte-identical, 940 bytes,
  sha256 `a7bf0df4deece328e11d22cccd2699ce580c6d025dc85cd3fd2e25608be2ff3c`.
- `list --limit 20 --offset 5 --format text`: byte-identical, 940 bytes,
  sha256 `bffe38736aeb11ae273d885c02e89ccc63718d69f6899f2677bf43e9a7e1a058`.
- `list --limit 0 --format text`: byte-identical, 564000 bytes,
  sha256 `9f171ea3d9114c797c0b04c0dc23d423efa72599992a8edff18c4c0480c1b251`.
- `list --limit 20 --long --format text`: byte-identical, 2699 bytes,
  sha256 `c87f6c029ffe521f77c9f80cf692270b49c2d0643f0d42e43ba0b12e86b7df2e`.

Measurements:

- `list --limit 20 --format text`: old `245.3 ms +/- 4.1 ms`, new
  `170.1 ms +/- 3.0 ms`, `1.44x` faster.
- `list --limit 20 --quiet`: old `242.0 ms +/- 5.1 ms`, new
  `171.5 ms +/- 5.2 ms`, `1.41x` faster.

Artifacts:

- `list-text-limit-old-vs-projected.json`
- `list-text-limit-quiet-old-vs-projected.json`
