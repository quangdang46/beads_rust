# Default label count probe

Date: 2026-05-04
Bead: beads_rust-72yf.16

## Corpus

- Workspace: `/data/tmp/br-read-matrix-20260504-aTl0u9`
- Source corpus: `/data/tmp/br-parallel-export-work-20260504T0628/.beads/issues.jsonl`
- Shape: 12,000 visible issues, 36,000 label rows

## Candidate

Specialize the default-visible `count --by label --json` path so label grouping
does not fall through the generic filtered label-count path.

Two variants were tested and rejected:

- Direct label grouping plus no-label anti-join: output-identical but slower.
- Left-join grouping with scalar default-visible total: output-identical but slower.

## Proof

Output equality:

```text
f893044d40ff4cf9c5aa354897c9c4fdd6ab69cd860e85c7a5d8c3f713a1c2de  count-label-baseline.json
f893044d40ff4cf9c5aa354897c9c4fdd6ab69cd860e85c7a5d8c3f713a1c2de  count-label-candidate.json
```

Timing for the better of the two rejected variants:

```text
baseline:  287.7 ms +/- 7.1 ms
candidate: 323.5 ms +/- 8.9 ms
```

The accepted prior binary was 1.12x faster than the candidate, so no source
changes were retained.
