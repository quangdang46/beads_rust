# broad default search page optimization

Bead: `beads_rust-72yf.29`

Dataset: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Control binary: `/data/tmp/br_72yf28_candidate2_br` (`dbb84409`)

Retained candidate binary: `/data/tmp/br_72yf29_candidate2_br`

## Retained result

The retained implementation adds a conservative default-visible first-page search
path for `limit > 0`, `offset == 0`, and no explicit filters. It queries the
critical-priority bucket first and, if that does not fill the page, finishes with
one ordered tail query for the remaining priorities. Other filter shapes keep the
generic search path.

| Command | Control | Candidate | Result |
| --- | ---: | ---: | ---: |
| `search payload --json` | 211.4 ms +/- 3.4 | 138.5 ms +/- 3.0 | 1.53x faster |
| `search payload --format toon` | 216.1 ms +/- 4.2 | 142.0 ms +/- 1.3 | 1.52x faster |
| `search zzz-no-match --json` | 65.2 ms +/- 0.5 | 80.2 ms +/- 4.2 | 1.23x slower |

The sparse no-match case regresses by about 15 ms because it now pays the
critical-priority preflight before the ordered tail query. This was accepted for
this slice because the swarm read-matrix target is the broad first-page search,
and the sparse case remains below 100 ms. A second warmup-heavy guard run measured
67.5 ms +/- 2.2 for control and 79.7 ms +/- 1.2 for candidate.

## Behavior proof

All output pairs passed `cmp`.

| Output | SHA-256 |
| --- | --- |
| `control-search-payload.json` | `2953a7defa431547a9bb3acb7504405f1a3ba3c68b969c1c87ef70cfd348b92b` |
| `candidate-search-payload.json` | `2953a7defa431547a9bb3acb7504405f1a3ba3c68b969c1c87ef70cfd348b92b` |
| `control-search-payload.toon` | `8d826220d81b19df8d987da98a6cd4097263e3280957e975ed82a5c2e916e9c5` |
| `candidate-search-payload.toon` | `8d826220d81b19df8d987da98a6cd4097263e3280957e975ed82a5c2e916e9c5` |
| `control-search-no-match.json` | `37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570` |
| `candidate-search-no-match.json` | `37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570` |

Focused storage proof:

```text
cargo test test_search_issues_default_visible_limited_page_matches_generic_order -- --nocapture
```

## Rejected candidate

The first candidate scanned every priority bucket separately. It produced the
same broad search win but regressed `search zzz-no-match --json` from 65.8 ms to
105.6 ms, so it was replaced with the critical-bucket-plus-tail version.
