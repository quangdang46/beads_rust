# Scheduler Candidate Hydration Probe

## Scope

Rejected optimization probes for `br scheduler --json --candidate-limit 512 --limit 20`
on `/data/tmp/br-read-matrix-20260504-aTl0u9`.

## Baseline

Baseline binary: `/data/tmp/br-candidate-default-label-count-20260504-local`

Post-count matrix timing for scheduler:

- `219.5 ms +/- 3.3 ms` in the six-command read matrix
- `222.2 ms +/- 9.5 ms` in the narrow-hydration paired run
- `221.7 ms +/- 7.8 ms` in the deferred-rationale paired run

## Probe 1: Narrow Candidate Hydration

Candidate binary: `/data/tmp/br-candidate-scheduler-narrow-20260504-local`

Change tested:

- Load scheduler candidates with a narrow scoring projection.
- Score and truncate candidates.
- Rehydrate returned ids with the ready-command projection before serialization.

Normalized output proof:

- `jq -S 'del(.generated_at)'` output matched baseline.
- Normalized SHA: `fe3effaa29575e563ef2eddfa9f05f288e54281902d759de69929f0b97f82eb9`

Timing:

- Baseline: `222.2 ms +/- 9.5 ms`
- Candidate: `219.5 ms +/- 9.7 ms`
- Result: `1.01x +/- 0.06x`, rejected as noise.

## Probe 2: Deferred Rationale Construction

Candidate binary: `/data/tmp/br-candidate-scheduler-rationale-defer-20260504-local`

Change tested:

- Keep the existing scheduler candidate hydration.
- Delay rationale string allocation until after `--limit` truncation.

Normalized output proof:

- `jq -S 'del(.generated_at)'` output matched baseline.
- Normalized SHA: `688196f979bdcff9498ae945e083d4c5d870f533a8eab8285d84e5e0a50896e0`

Timing:

- Baseline: `221.7 ms +/- 7.8 ms`
- Candidate: `225.2 ms +/- 6.8 ms`
- Result: baseline was `1.02x +/- 0.05x` faster, rejected.

## Decision

Both code probes were reverted. Scheduler latency is not currently dominated by
candidate-row width or rationale string construction at this workload size.
