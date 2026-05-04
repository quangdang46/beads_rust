# br scheduler metadata threshold - 2026-05-04

Workload: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Baseline binary: `/data/tmp/br-scheduler-threshold-baseline-256-20260504`

Candidate binary: `/data/tmp/br-scheduler-threshold-candidate-96-20260504`

Change: lower `SCHEDULER_FULL_METADATA_THRESHOLD` from 256 to 96.
Scheduler candidate windows at `--candidate-limit >= 96` now reuse the full
relation metadata path. Smaller windows keep the existing medium relation
queries.

## Behavior proof

Baseline and candidate scheduler JSON match after normalizing the expected
volatile fields:

- `generated_at`
- `recommendations[].evidence.stale_claim.updated_age_minutes`

Covered commands:

- `br scheduler --json`
- `br scheduler --json --candidate-limit 95`
- `br scheduler --json --candidate-limit 96`
- `br scheduler --json --candidate-limit 100`
- `br scheduler --json --candidate-limit 255`
- `br scheduler --json --candidate-limit 256`
- `br scheduler --json --candidate-limit 0`

See `normalized-sha256.txt` for captured normalized hashes.

## Timing

Primary paired run: `hyperfine.json`

| Command | Before | After | Result |
| --- | ---: | ---: | --- |
| `scheduler --json` | 235.2 ms +/- 2.7 | 234.9 ms +/- 4.7 | unchanged default |
| `scheduler --json --candidate-limit 95` | 316.0 ms +/- 3.1 | 319.4 ms +/- 2.9 | unchanged control, same path |
| `scheduler --json --candidate-limit 96` | 319.7 ms +/- 4.4 | 223.9 ms +/- 3.1 | 1.43x faster |
| `scheduler --json --candidate-limit 100` | 324.4 ms +/- 3.1 | 224.0 ms +/- 2.8 | 1.45x faster |
| `scheduler --json --candidate-limit 255` | 561.9 ms +/- 12.0 | 231.8 ms +/- 2.6 | 2.42x faster |

Control run: `hyperfine-unlimited.json`

| Command | Before | After | Result |
| --- | ---: | ---: | --- |
| `scheduler --json --candidate-limit 0` | 273.3 ms +/- 6.3 | 274.0 ms +/- 3.6 | unchanged unlimited full scan |
