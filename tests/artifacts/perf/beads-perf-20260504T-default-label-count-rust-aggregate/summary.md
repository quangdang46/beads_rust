# Default Label Count Rust Aggregation

## Scope

Optimize the default-visible `br count --by label --json` read path on the 12k
read-matrix workspace at `/data/tmp/br-read-matrix-20260504-aTl0u9`.

## Baseline

Baseline binary: `/data/tmp/br-candidate-list-page-toon-stream-20260504`

Before this slice, the read matrix showed this as the slowest remaining read
command:

- `count --by label --json`: `300.7 ms +/- 6.4 ms`
- `strace`: about `1,577 pread64`, `1,573 futex`
- `/usr/bin/time -v`: `156,788 KB` max RSS

## Candidate

Candidate binary: `/data/tmp/br-candidate-default-label-count-20260504-local`

The candidate keeps filtered label counts on the existing SQL fallback and adds
a default-visible fast path:

- Query default-visible issue ids once.
- Query labels ordered by `(issue_id, label)`.
- Aggregate labels in Rust with a visible-id set.
- Add the `(no labels)` bucket from `total - labeled_visible_issues`.

## Isomorphism Proof

Command:

```bash
cd /data/tmp/br-read-matrix-20260504-aTl0u9
/data/tmp/br-candidate-list-page-toon-stream-20260504 count --by label --json > baseline.json
/data/tmp/br-candidate-default-label-count-20260504-local count --by label --json > candidate.json
sha256sum baseline.json candidate.json
cmp -s baseline.json candidate.json
```

Both outputs hashed to:

```text
f893044d40ff4cf9c5aa354897c9c4fdd6ab69cd860e85c7a5d8c3f713a1c2de
```

## Timing Proof

Paired `hyperfine --warmup 3 --runs 10`:

- Baseline: `302.3 ms +/- 9.6 ms`
- Candidate: `118.6 ms +/- 1.9 ms`
- Speedup: `2.55x +/- 0.09x`

## Candidate Resource Snapshot

`/usr/bin/time -v`:

- Wall: `0.12 s`
- User: `0.07 s`
- System: `0.04 s`
- Max RSS: `76,876 KB`

`strace -qq -c`:

- `1,559 pread64`
- `1,571 futex`
- `63 madvise`
- `3,501` total syscalls
