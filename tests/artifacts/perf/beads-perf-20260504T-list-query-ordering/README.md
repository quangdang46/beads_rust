# list default structured page query ordering

Date: 2026-05-04
Bead: beads_rust-9suj
Corpus: `/data/tmp/br-read-matrix-20260504-aTl0u9`
Baseline binary: `/data/tmp/br-candidate-default-label-count-20260504-local`
Candidate binary: `/data/tmp/br-target-list-query-release/release/br`

## Change

`list_issues` now uses a bounded default-visible first-page path when filters
are otherwise default, `limit > 0`, and `offset == 0`. It reads full issue rows
priority bucket by priority bucket until the requested page is full, preserving
the existing `priority ASC, created_at DESC, id ASC` ordering without hydrating
and sorting the full visible issue table.

The path is disabled for explicit filters, explicit sort/reverse, offsets,
closed-inclusive lists, labels, title filters, assignee filters, and unbounded
`--limit 0` lists.

## Equivalence

- `list --limit 50 --json`: baseline and candidate both hash to
  `cbec91ae42b8f062ebbffb3ac562b58847057ba56a472d251429d11df44ba1db`.
- `list --limit 50 --format toon`: baseline and candidate both hash to
  `b59f7289e25b8ff4f1dd31e2055440dbc5e5362cae09edd7ba39e9e700d81a38`.
- `list --limit 0 --json` remains byte-identical at
  `5966f6c830c9f83f05ad98185a711b747e591a1774b4047f76c9c2b231cb63c1`.

## Timing

- `list --limit 50 --json`: 193.1 ms +/- 5.4 ms baseline to
  145.3 ms +/- 3.6 ms candidate, 1.33x +/- 0.05x faster.
- `list --limit 1 --json`: 123.5 ms +/- 4.1 ms baseline to
  72.1 ms +/- 3.6 ms candidate, 1.71x +/- 0.10x faster.
- `list --limit 50 --format toon`: 194.4 ms +/- 5.7 ms baseline to
  146.1 ms +/- 3.3 ms candidate, 1.33x +/- 0.05x faster.

## Resource Sample

Single `/usr/bin/time -v` sample for `list --limit 50 --json`:

- Baseline: elapsed 0.19 s, user 0.17 s, system 0.01 s, max RSS 32864 KB.
- Candidate: elapsed 0.14 s, user 0.13 s, system 0.01 s, max RSS 25700 KB.

Single `strace -qq -c` sample for `list --limit 50 --json`:

- Baseline: 3542 total syscalls, 46 errors.
- Candidate: 3544 total syscalls, 46 errors.

The win is CPU/RSS from bounded row hydration, not syscall reduction.
