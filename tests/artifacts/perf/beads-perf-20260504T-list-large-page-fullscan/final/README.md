# Large Structured List Page Full Scan

Bead: `beads_rust-72yf.32`

Date: 2026-05-04

## Goal

Speed broad default-visible structured `br list` first pages on large
workspaces. The baseline anomaly on `/data/tmp/br-read-matrix-20260504-aTl0u9`
was that `list --limit 0 --json` over all 12k visible issues was faster than
`list --limit 150 --json` and `list --limit 200 --json`.

## Accepted Plan

For JSON and TOON default first pages with `limit >= 128`, use the same full
default-visible scan plan as unlimited structured output, then truncate to the
requested page before writing output. The plan also uses the matching full
relation-metadata scan; the first probe kept batched relation metadata and was
rejected as slower.

Small pages, filtered pages, offset pages, text output, and CSV output remain on
their previous plans.

## Output Proof

The candidate was byte-identical to `/data/tmp/br_72yf30_candidate_br` on the
large matrix:

| Command | Bytes | SHA-256 |
| --- | ---: | --- |
| `list --limit 50 --json` | 29,265 | `cbec91ae42b8f062ebbffb3ac562b58847057ba56a472d251429d11df44ba1db` |
| `list --limit 127 --json` | 74,234 | `9e9ff97d29e2eac9561e9db2689ac8685880b690924265df6750cc5a863865cc` |
| `list --limit 128 --json` | 74,818 | `7eaee9d359ac48d8a3eeeae07e4fb6908bf0bc4176a8dc45be685be922164aac` |
| `list --limit 150 --json` | 87,666 | `17135cd156e79e204b70ccda38b5d8ab40ed51c6906b60021828c203d2b801bb` |
| `list --limit 200 --json` | 116,866 | `b2b91c8d167315ac68655337b06f0f722011da58c21f15a7df9e4abd5e5d0a16` |
| `list --limit 128 --format toon` | 78,014 | `2774f451b0912103c26f7a13658a48ea63c5cab55ec1260b184d84b4d5985b39` |
| `list --limit 200 --format toon` | 121,862 | `29e41cf3753427005d7c27068e53d97ac8ee13a7a157245fe8e0ae7db631ae53` |

Focused unit proof:

```bash
env CARGO_TARGET_DIR=/data/tmp/br_72yf32_local_target \
  cargo test test_large_structured_pages_use_full_default_scan -- --nocapture
```

## Timing

Final paired `hyperfine --warmup 2 --runs 7`:

| Command | Baseline | Candidate | Speedup |
| --- | ---: | ---: | ---: |
| `list --limit 50 --json` | 148.0 +/- 4.1 ms | 147.3 +/- 3.8 ms | flat |
| `list --limit 127 --json` | 259.1 +/- 4.4 ms | 260.1 +/- 4.9 ms | flat |
| `list --limit 128 --json` | 256.7 +/- 7.0 ms | 176.9 +/- 4.8 ms | 1.45x |
| `list --limit 150 --json` | 292.2 +/- 7.2 ms | 177.7 +/- 5.4 ms | 1.64x |
| `list --limit 200 --json` | 365.4 +/- 8.6 ms | 177.0 +/- 3.5 ms | 2.06x |
| `list --limit 200 --format toon` | 367.8 +/- 5.0 ms | 176.1 +/- 3.4 ms | 2.09x |

Raw artifacts:

- `final-hyperfine.json`
- `final-hyperfine.md`
- `hyperfine.json` and `hyperfine.md` preserve the rejected batched-relation
  first probe.

## Decision

Accepted. The threshold prevents small-page regressions, the threshold boundary
has byte-identical output, and large structured pages now use the lower-latency
full scan plus full relation metadata plan.
