# Startup Cache Perf Evidence

Slice: `beads_rust-72yf.6` optimistic route/config startup cache.

Binary:
`/data/tmp/beads_rust_scheduler_final/release/br`

Workspace:
`/data/tmp/br-startup-cache-bench-183146-1614099/project/deep/nested`

Workload:
`br where --json` from a nested project directory with a generated `.beads/config.yaml`
containing 16,000 startup config entries plus metadata and routes files. The
opt-in candidate was warmed once with `BR_STARTUP_CACHE=1` and
`BR_STARTUP_CACHE_DIR=/data/tmp/br-startup-cache-dir-183146-1614099`.

Result:

| Mode | Mean | Min | Max |
| --- | ---: | ---: | ---: |
| Direct startup load | 170.4 ms | 162.9 ms | 185.7 ms |
| Warm opt-in startup cache | 39.7 ms | 37.2 ms | 44.9 ms |

Hyperfine reported the warm opt-in cache as `4.29 +/- 0.21` times faster.

Correctness:

`direct-output.json` and `cached-output.json` have matching SHA-256:
`f53eacc77688aac10afb998025cc0b88f40a534f8de0b662a4ae70575f9c62e2`.

Notes:

- The cache is opt-in through `BR_STARTUP_CACHE=1`; the default startup path is unchanged.
- Cache hits validate metadata, project/user config, local routes, town routes,
  redirect, environment, and DB override witnesses before returning.
- The strace summaries show lower measured traced syscall time on the cached run
  while adding extra `statx` witness checks; the wall-clock win comes from
  avoiding large YAML config parsing on stable startup metadata.
