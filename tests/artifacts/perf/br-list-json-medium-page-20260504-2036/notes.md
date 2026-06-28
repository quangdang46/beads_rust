# br list JSON medium-page threshold - 2026-05-04

Workload: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Baseline binary: `/data/tmp/br-target-stats-summary/release/br`

Candidate binary: `/data/tmp/br-target-list-threshold/release/br`

Change: lower `LARGE_STRUCTURED_LIST_FULL_SCAN_THRESHOLD` from 128 to 96.
Default-visible structured pages at `limit >= 96` now reuse the existing full
default-visible scan/relation metadata path. Smaller pages keep the existing
medium-page relation queries.

## Behavior proof

Baseline and candidate JSON are byte-identical for:

- `br list --json --limit 10`
- `br list --json --limit 64`
- `br list --json --limit 95`
- `br list --json --limit 96`
- `br list --json --limit 100`
- `br list --json --limit 0`

See `golden-sha256.txt` for the captured output hashes.

## Timing

Primary paired run: `hyperfine-threshold96.json`

| Command | Before | After | Result |
| --- | ---: | ---: | --- |
| `list --json --limit 64` | 200.4 ms +/- 32.7 | 167.6 ms +/- 3.2 | noisy control, same code path |
| `list --json --limit 95` | 219.0 ms +/- 5.8 | 223.6 ms +/- 5.4 | unchanged control, same code path |
| `list --json --limit 96` | 219.6 ms +/- 6.7 | 180.8 ms +/- 3.7 | 1.21x faster |
| `list --json --limit 100` | 223.4 ms +/- 2.5 | 182.3 ms +/- 1.6 | 1.23x faster |

## No-go recorded during the pass

An initial threshold of 64 improved `limit 100` but regressed the exact
`limit 64` boundary from 172.2 ms to 185.6 ms in `hyperfine.json`. The committed
candidate uses 96 so the lower medium-page range stays on the old path.
