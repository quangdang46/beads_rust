# NUMA Read-Command Profile - 2026-05-04

This bundle records the pilot evidence for `beads_rust-72yf.39`.
It changes no runtime behavior. The purpose is to make future read-path work
hardware-aware before more code probes are attempted.

## Host and Corpus

- Host: AMD Ryzen Threadripper PRO 5995WX, 64 cores / 128 logical CPUs.
- Memory: 536,069,881,856 bytes total, with 462,368,509,952 bytes available at capture time.
- NUMA: one kernel-visible node. Cross-node binding is therefore unavailable on this host.
- Binary: `/data/tmp/cargo_target_beads_rust_pinktiger_numa/release/br`, `br 0.2.5`, release build without default features.
- Corpus: `/data/tmp/br-read-matrix-20260504-aTl0u9`, 12,000 issues.

Raw environment files are under `env/`; `env.json` is the machine-readable
summary. The command output hashes are in `golden/command-output-sha256.txt`.

## Timing Matrix

Default scheduler placement:

| Command | Mean ms | P50 ms | P95 ms | P99 ms | User ms | System ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `list --json --limit 100` | 191 | 192 | 198 | 198 | 138 | 53 |
| `ready --json --limit 100` | 143 | 142 | 151 | 151 | 106 | 36 |
| `scheduler --json --candidate-limit 100` | 231 | 232 | 235 | 235 | 185 | 45 |
| `search agent --json --limit 100` | 68 | 67 | 76 | 76 | 54 | 14 |
| `stats --no-activity --json` | 127 | 127 | 134 | 134 | 88 | 39 |
| `label list-all --json` | 81 | 82 | 85 | 85 | 43 | 38 |

Pinned to logical CPU 0 with `taskset -c 0`:

| Command | Mean ms | P50 ms | P95 ms | P99 ms | User ms | System ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `list --json --limit 100` | 211 | 204 | 247 | 247 | 148 | 63 |
| `ready --json --limit 100` | 170 | 160 | 235 | 235 | 128 | 41 |
| `scheduler --json --candidate-limit 100` | 239 | 236 | 264 | 264 | 191 | 47 |
| `search agent --json --limit 100` | 75 | 73 | 82 | 82 | 56 | 18 |
| `stats --no-activity --json` | 139 | 140 | 152 | 152 | 95 | 44 |
| `label list-all --json` | 87 | 84 | 96 | 96 | 43 | 43 |

Raw hyperfine samples are in `timing/hyperfine-default.json` and
`timing/hyperfine-pinned-cpu0.json`; summarized p50/p95/p99 files are beside
them.

## Syscall and Page-Read Evidence

The pilot read commands all hit a similar SQLite page-read floor:

| Command | `pread64` calls | `futex` calls | `statx` calls | `openat` calls |
| --- | ---: | ---: | ---: | ---: |
| `list --json --limit 100` | 1583 | 1568 | 84 | 21 |
| `ready --json --limit 100` | 1365 | 1365 | 94 | 21 |
| `scheduler --json --candidate-limit 100` | 1593 | 1578 | 114 | 21 |
| `search agent --json --limit 100` | 1348 | 1354 | 64 | 21 |
| `stats --no-activity --json` | 1388 | 1366 | 104 | 21 |
| `label list-all --json` | 1572 | 1567 | 74 | 21 |

Full `strace -c` outputs are under `syscalls/`.

## Tail Decomposition

- Queueing/lock: read commands ran with `--no-auto-import --no-auto-flush`, so
  this pilot intentionally excludes write-lock queueing. Pair this profile with
  `bench_contention_replay` for lock contention work.
- Service CPU: scheduler is the highest CPU service cost in this matrix; list is
  next. Pinned CPU 0 widens tails most for ready/list/scheduler, so placement
  variance is real even without cross-node NUMA.
- IO/page reads: every command performs roughly 1.3k-1.6k `pread64` calls. The
  next justified read-path work should attack storage page reads or relation
  hydration, not only wrapper metadata calls.
- Serialization/output: output sizes range from 3 bytes for an empty search to
  58,466 bytes for `list --limit 100`; serialization is relevant for list/ready
  but does not explain the shared syscall floor.

## Future Lever Guidance

This bundle justifies focusing on:

- storage-level read-page reduction for list/scheduler/label list-all;
- relation hydration paths that cause similar `pread64` and `futex` counts;
- scheduler CPU-service reductions after page-read work is isolated;
- a true cross-NUMA run on hosts where `numactl --hardware` reports at least two
  nodes.
