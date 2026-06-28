# Scheduler Slice Evidence

This bundle records the behavior and timing evidence for `beads_rust-72yf.4`
after adding the first `br scheduler` slice.

## Commands

- Baseline ready parity:
  - `/tmp/br-before-scheduler-slice --no-auto-import --no-auto-flush ready --limit 0 --json`
  - `/tmp/rch_target_beads_rust_dustypuma_blocked_projection_local/release/br --no-auto-import --no-auto-flush ready --limit 0 --json`
- Scheduler timing:
  - `/tmp/rch_target_beads_rust_dustypuma_blocked_projection_local/release/br --no-auto-import --no-auto-flush scheduler --json --limit 20`
- Scheduler syscall profile:
  - `strace -c -o /tmp/br-scheduler-strace.txt /tmp/rch_target_beads_rust_dustypuma_blocked_projection_local/release/br --no-auto-import --no-auto-flush scheduler --json --limit 20`

## Files

- `ready-before.json` and `ready-after.json`: full old/new `ready --json`
  outputs.
- `ready-output-hashes.txt`: SHA-256 proof that old and new `ready --json`
  outputs are byte-identical.
- `br-scheduler-ready-before-after.json`: hyperfine timing for old/new
  `ready --json`.
- `br-scheduler-command-hyperfine.json`: hyperfine timing for the new
  scheduler command.
- `br-scheduler-output.json`: representative scheduler JSON output.
- `br-scheduler-strace.txt`: scheduler syscall summary.
- `hashes.txt`: binary and scheduler-output hashes.

## Result

The ready-command output hash stayed identical:

`cdfe53f5ce2039d9362207385cc42af6be8099f4cb457811e26903429b8b3521`

The ready before/after timing was noisy. Minimum runtime stayed effectively flat
at 24.1 ms before and 24.2 ms after, while the mean moved from 36.2 ms to
45.9 ms with high variance. This slice is therefore recorded as a scheduler
capability addition with behavior parity, not as a ready-path performance win.

The new scheduler command measured a 27.4 ms minimum and 59.5 ms mean on this
repo snapshot. Its output uses schema `br.scheduler.v1` and includes bounded
candidate selection, deterministic fallback rank, integer score evidence, and
human-readable rationale strings for swarm scheduling decisions.
