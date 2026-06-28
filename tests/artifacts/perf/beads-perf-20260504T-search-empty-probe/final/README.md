# Search Empty-Probe Follow-Up

Slice: `beads_rust-72yf.30`

Control binary: `/data/tmp/br_72yf29_candidate2_br` (`759b31f9`)

Candidate binary: `/data/tmp/br_72yf30_candidate_br`

Dataset: `/data/tmp/br-read-matrix-20260504-aTl0u9`

## Change

`beads_rust-72yf.29` added a priority-window fast path for default-visible
limited search pages. That path improved broad payload searches but made sparse
no-match searches scan both the critical-priority window and the tail window.

This follow-up adds a conservative unsorted existence probe inside the same
default-visible fast path. If no visible issue matches the query, search returns
`[]` after one scan. If a match exists, the command continues through the
priority-window path that preserves `priority ASC, created_at DESC, id ASC`.

## Timing

Paired candidate run:

| Command | Control | Candidate | Result |
| --- | ---: | ---: | --- |
| `search payload --json` | 144.1 ms +/- 3.6 | 146.4 ms +/- 6.4 | flat/noise |
| `search payload --format toon` | 142.3 ms +/- 1.2 | 144.1 ms +/- 4.6 | flat/noise |
| `search zzz-no-match --json` | 79.3 ms +/- 2.0 | 66.5 ms +/- 1.7 | 1.19x faster |

Fresh shipped-state baseline before editing:

| Command | Mean |
| --- | ---: |
| `search payload --json` | 136.1 ms +/- 3.4 |
| `search payload --format toon` | 141.3 ms +/- 5.2 |
| `search zzz-no-match --json` | 80.5 ms +/- 4.3 |

## Output Equality

All control/candidate output pairs passed `cmp`.

SHA-256:

```text
2953a7defa431547a9bb3acb7504405f1a3ba3c68b969c1c87ef70cfd348b92b  control-search-payload.json
2953a7defa431547a9bb3acb7504405f1a3ba3c68b969c1c87ef70cfd348b92b  candidate-search-payload.json
8d826220d81b19df8d987da98a6cd4097263e3280957e975ed82a5c2e916e9c5  control-search-payload.toon
8d826220d81b19df8d987da98a6cd4097263e3280957e975ed82a5c2e916e9c5  candidate-search-payload.toon
37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570  control-search-no-match.json
37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570  candidate-search-no-match.json
```

## Verification

```text
cargo test test_search_issues_default_visible_limited_page_matches_generic_order -- --nocapture
```
