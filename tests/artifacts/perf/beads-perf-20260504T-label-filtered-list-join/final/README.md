# Label-Filtered List Query Measurement

Dataset: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Baseline binary: `/data/tmp/br-baseline-label-filtered-list-20260504-before-hzq3`

Candidate binary: `/data/tmp/br-target-list-query-release/release/br`

The final change avoids fsqlite's slower label `IN (SELECT ...)` list path by
materializing label candidate IDs in Rust before the issue-page query. For the
common structured default-visible single-label case, the exact total is counted
from default-visible issue IDs plus matching label rows.

Paired `hyperfine --warmup 1 --runs 3` results:

| Command | Baseline | Candidate | Speedup |
| --- | ---: | ---: | ---: |
| `list --limit 50 --json --label export` | 7.520 s +/- 0.112 | 3.395 s +/- 0.013 | 2.21x |
| `list --limit 50 --format toon --label export` | 8.032 s +/- 0.027 | 3.316 s +/- 0.015 | 2.42x |
| `list --limit 50 --no-color --label export` | 3.759 s +/- 0.093 | 3.190 s +/- 0.059 | 1.18x |
| `list --limit 50 --json --label lane-00` | 370.9 ms +/- 1.7 | 253.5 ms +/- 2.6 | 1.46x |

Golden output checks:

- `list --limit 50 --json --label export`
- `list --limit 50 --json --label lane-00`
- `list --limit 50 --json --label export --label lane-00`
- `list --limit 50 --format toon --label export`

Each baseline/candidate pair matched byte-for-byte with `cmp`; hashes are in
`golden-output-sha256.txt`.
