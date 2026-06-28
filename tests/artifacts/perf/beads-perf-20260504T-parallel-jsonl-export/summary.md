# Parallel JSONL Export Line Preparation

Run date: 2026-05-04
Commit under test: uncommitted candidate after `7abef40a`
Workload: `/data/tmp/br-parallel-export-work-20260504T0628`

## Workload

- 12,000 synthetic issues in one `.beads/issues.jsonl`.
- Each issue has labels, an assignee, a description, and one comment.
- Imported with the release `br` binary, then measured using forced JSONL export:
  `br sync --flush-only --force --json`.

## Baseline

- Binary: `/data/tmp/beads_rust_parallel_export/release/br` at `7abef40a`.
- Hyperfine: 721.45 ms mean, 20.69 ms stddev, 5 runs.
- `/usr/bin/time -v`: 0.71 s elapsed, 0.56 s user, 0.10 s system, max RSS 137,344 KB.
- `strace -c`: 54.47 ms in syscalls, so the path was mostly CPU work.
- JSON stdout hash was stable across baseline probes:
  `5c1c60748324748e0591f5319e5b2523fb8cd2be750a992b1b00e7361e4d3b82`.
- Exported JSONL SHA-256:
  `30eb851908afb1a054da8248c6e406d79d2cc8caabc135d4c61e1326a4a7cf8a`.

## Candidate

Change: prepare JSONL issue lines and per-issue content hashes in bounded worker chunks, then write and hash the prepared lines in original issue order. This is a morsel-style parallel stage with a serial ordered gather.

- Serial fallback command:
  `br sync --flush-only --force --export-parallelism 1 --json`.
- Parallel command:
  `br sync --flush-only --force --export-parallelism 64 --json`.
- Hyperfine serial fallback: 725.94 ms mean, 4.50 ms stddev, 5 runs.
- Hyperfine parallel: 686.04 ms mean, 5.30 ms stddev, 5 runs.
- Candidate speedup against serial fallback: 1.058x.
- Candidate speedup against pre-change baseline: 1.052x.
- `/usr/bin/time -v` parallel: 0.69 s elapsed, 0.59 s user, 0.11 s system, max RSS 138,292 KB.
- Serial and parallel JSON stdout hashes were identical:
  `5c1c60748324748e0591f5319e5b2523fb8cd2be750a992b1b00e7361e4d3b82`.
- Serial and parallel exported JSONL SHA-256 values were identical:
  `30eb851908afb1a054da8248c6e406d79d2cc8caabc135d4c61e1326a4a7cf8a`.

## Isomorphism Proof

- Ordering preserved: yes. Worker chunks are reassembled by original start index; writing and final SHA-256 hashing remain serial in issue order.
- Tie-breaking unchanged: yes. Export IDs are still sorted before hydration, and hydrated batches keep the same ID order.
- Floating point: N/A.
- RNG seeds: N/A.
- Golden outputs: serial fallback and parallel export produced byte-identical JSONL and identical JSON stdout.

## Fallback

- `--export-parallelism 1` forces the old serial preparation shape.
- `BR_DISABLE_PARALLEL_JSONL_EXPORT=1` disables the parallel stage for all file exports.
- Parallelism auto mode is capped at 64 workers and host `available_parallelism()`.
