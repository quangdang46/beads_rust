# List TOON Iterator Stream Probe

## Scope

Rejected optimization probe for `br list --limit 0 --format toon` on
`/data/tmp/br-read-matrix-20260504-aTl0u9`.

## Baseline

Baseline binary: `/data/tmp/br-candidate-default-label-count-20260504-local`

The post-count read matrix measured `list --limit 0 --format toon` at
`212.3 ms +/- 10.3 ms`.

## Candidate

Candidate binary: `/data/tmp/br-candidate-list-toon-iter-20260504-local`

Change tested:

- Add an iterator-based TOON list-page writer.
- Use it for stats-off `list --format toon` when base rows have no
  dependencies/comments.
- Preserve the existing materialized fallback for stats or richer relation
  payloads.

## Isomorphism Proof

Both outputs were byte-for-byte identical:

- SHA: `e09e19235b3fd1b0a3956724a4bbd79c8586198e5419b69b5ef0423a2f4244a7`
- Size: `7,308,063` bytes

## Timing

Paired `hyperfine --warmup 3 --runs 10`:

- Baseline: `212.8 ms +/- 6.9 ms`
- Candidate: `214.1 ms +/- 4.7 ms`
- Result: baseline was `1.01x +/- 0.04x` faster.

## Decision

The code probe was reverted. `list --format toon` is not currently dominated by
the final `Vec<IssueWithCounts>` materialization at this workload size.
