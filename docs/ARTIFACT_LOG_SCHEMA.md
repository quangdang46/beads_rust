# Artifact Log Schema

> Machine-parseable JSONL schema for E2E test artifacts.
> Task: beads_rust-r23m

## Overview

The E2E test harness emits structured logs in JSONL format for:
1. Command execution events
2. File tree snapshots
3. Test run summaries

All artifacts are written to `target/test-artifacts/<suite>/<test>/`.

## Schema Definitions

### 1. RunEvent (events.jsonl)

Each line in `events.jsonl` is a JSON object matching this schema:

```json
{
  "timestamp": "2026-01-17T12:34:56.789Z",  // RFC3339, required
  "event_type": "command",                   // "command" | "snapshot", required
  "label": "init",                           // human-readable step name, required
  "binary": "br",                            // binary executed, required for command
  "args": ["init", "--prefix", "bd"],        // array of strings, required
  "cwd": "/tmp/test123",                     // working directory, required
  "exit_code": 0,                            // integer, required for command
  "success": true,                           // boolean, required
  "duration_ms": 42,                         // integer (milliseconds), required for command
  "stdout_len": 1024,                        // byte count, required
  "stderr_len": 0,                           // byte count, required
  "stdout_path": "0001_init.stdout",         // optional, relative path
  "stderr_path": null,                       // optional, relative path
  "snapshot_path": null                      // optional, for event_type=snapshot
}
```

#### Field Definitions

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `timestamp` | string | Yes | RFC3339 timestamp (UTC) |
| `event_type` | string | Yes | `"command"` or `"snapshot"` |
| `label` | string | Yes | Human-readable step identifier |
| `binary` | string | Yes* | Binary name (`"br"`, `"bd"`, `"git"`) |
| `args` | string[] | Yes | Command arguments (excluding binary) |
| `cwd` | string | Yes | Absolute path to working directory |
| `exit_code` | integer | Yes* | Process exit code (0 = success) |
| `success` | boolean | Yes | True if exit_code == 0 |
| `duration_ms` | integer | Yes* | Execution time in milliseconds |
| `stdout_len` | integer | Yes | Byte count of stdout |
| `stderr_len` | integer | Yes | Byte count of stderr |
| `stdout_path` | string? | No | Relative path to stdout capture file |
| `stderr_path` | string? | No | Relative path to stderr capture file |
| `snapshot_path` | string? | No | Relative path to snapshot JSON |

*Required when `event_type` is `"command"`.

### 2. FileEntry (*.snapshot.json)

Snapshot files contain an array of file entries:

```json
[
  {"path": ".beads", "size": 0, "is_dir": true},
  {"path": ".beads/beads.db", "size": 12288, "is_dir": false},
  {"path": ".beads/issues.jsonl", "size": 456, "is_dir": false}
]
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `path` | string | Yes | Path relative to workspace root |
| `size` | integer | Yes | File size in bytes (0 for dirs) |
| `is_dir` | boolean | Yes | True if directory |

### 3. Summary (summary.json)

Written at test completion:

```json
{
  "suite": "e2e_basic",
  "test": "test_create_issue",
  "passed": true,
  "run_count": 5,
  "timestamp": "2026-01-17T12:35:00.000Z"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `suite` | string | Yes | Test suite name |
| `test` | string | Yes | Test function name |
| `passed` | boolean | Yes | Overall test result |
| `run_count` | integer | Yes | Number of commands executed |
| `timestamp` | string | Yes | RFC3339 completion timestamp |

### 4. Startup Matrix Manifest (startup-matrix-manifest.json)

Storage-open and startup performance bundles include a manifest that makes each
startup state explicit and keeps raw evidence discoverable even when aggregation
fails.

```json
{
  "schema_version": "br.startup-matrix.v1",
  "matrix_name": "storage-open-smoke",
  "generated_at": "2026-05-03T01:00:00Z",
  "states": [
    {
      "state": "clean",
      "command_log_path": "logs/clean.log",
      "timing_summary_path": "timing/clean.json",
      "syscall_summary_path": "syscalls/clean.txt",
      "rss_summary_path": "rss/clean.json",
      "raw_artifact_paths": ["raw/clean.trace"]
    }
  ],
  "aggregation": {
    "status": "ok",
    "raw_evidence_preserved": true,
    "error": null
  }
}
```

Required `state` values are:

- `clean`
- `stale`
- `routed`
- `no_db`
- `read_only_fast_open`
- `sync_status`
- `recovery_anomaly`

Each state must reference a command log, timing summary, syscall summary, and RSS
summary using relative paths inside the bundle. Raw artifact references are
optional for successful aggregation, but partial or failed aggregation must set
`raw_evidence_preserved` to `true` and keep at least one raw artifact reference.

### 5. Performance Evidence Manifest (perf-evidence-manifest.json)

Reusable performance proof bundles include a manifest that ties command output
goldens to timing/resource evidence and an advisory or enforcing gate policy.
The schema is designed for optimization passes where a later change must prove
behavior preservation and bounded tail-latency/resource drift.

```json
{
  "schema_version": "br.perf-evidence.v1",
  "generated_at": "2026-05-03T02:00:00Z",
  "valid_until": "2026-06-02T02:00:00Z",
  "command": {
    "label": "list_json",
    "args": ["list", "--json"]
  },
  "dataset": {
    "name": "tiny-smoke",
    "issue_count": 3,
    "content_hash": "64-char-sha256"
  },
  "git": {
    "revision": "git-revision",
    "dirty": false
  },
  "binary": {
    "path": "target/debug/br",
    "version": "br 0.2.5"
  },
  "environment": {
    "os": "linux",
    "rustc": "rustc 1.91.0-nightly",
    "env": [{"name": "NO_COLOR", "value_hash": "64-char-sha256"}]
  },
  "timing": {
    "sample_count": 3,
    "min_ms": 1.0,
    "p50_ms": 2.0,
    "p95_ms": 3.0,
    "p99_ms": 3.0,
    "max_ms": 3.0,
    "summary_path": "timing/list.json",
    "raw_samples_path": "timing/list-samples.jsonl"
  },
  "resources": {
    "syscall_summary_path": "syscalls/list.json",
    "io_summary_path": "io/list.json",
    "rss_summary_path": "rss/list.json"
  },
  "golden": {
    "stdout_sha256": "64-char-sha256",
    "stderr_sha256": "64-char-sha256",
    "checksums_path": "golden/checksums.txt",
    "stdout_path": "golden/stdout",
    "stderr_path": "golden/stderr"
  },
  "isomorphism_note_path": "proof/isomorphism.md",
  "policy": {
    "mode": "enforcing",
    "baseline_manifest_path": "baseline/perf-evidence-manifest.json",
    "latency_regression_budget_pct": 5.0,
    "syscall_regression_budget_pct": 10.0,
    "output_hash_must_match": true
  },
  "comparison": {
    "status": "pass",
    "baseline_manifest_path": "baseline/perf-evidence-manifest.json",
    "p95_delta_pct": 0.0,
    "stdout_hash_match": true,
    "syscall_delta_pct": 0.0,
    "decision_reason": "candidate stayed within enforcing policy"
  },
  "raw_artifact_paths": ["raw/list-0.stdout", "raw/list-0.stderr"]
}
```

Policy modes:

- `advisory`: comparison failures produce warnings and do not block unrelated
  commands.
- `enforcing`: a baseline manifest path and passing comparison are required.

### 6. Sequential Drift Guard (br.sequential-drift-guard.v1)

Adaptive fast paths must have an inspectable safety rail before they are enabled
by runtime policy. The sequential drift guard is a pure policy-adjacent document:
callers provide every state, action, loss, proof, and evidence field, and the
evaluator returns either `accept_fast_path` or `disable_fast_path`.

The canonical Rust schema lives in `src/policy.rs` as `SequentialDriftGuard`:

```json
{
  "schema_version": 1,
  "guard_id": "scheduler-window-p95-guard",
  "policy_id": "swarm-scale-defaults",
  "state": {
    "operation": "scheduler",
    "corpus_hash": "sha256:corpus-a",
    "policy_revision": "policy-rev-1"
  },
  "action": {
    "action_id": "enable_scheduler_window_100",
    "fast_path_name": "scheduler_candidate_limit_100",
    "output_name": "candidate_window_enabled",
    "accept_value": true
  },
  "loss": {
    "max_p95_regression_pct": 5,
    "max_error_delta": 0,
    "min_sample_count": 10
  },
  "evidence": {
    "expected_corpus_hash": "sha256:corpus-a",
    "corpus_hash": "sha256:corpus-a",
    "replay_seed": 42,
    "sample_count": 10,
    "baseline_p95_ms": 240,
    "candidate_p95_ms": 230,
    "baseline_error_count": 0,
    "candidate_error_count": 0
  },
  "proof_obligations": {
    "deterministic_input_evidence": true,
    "no_hidden_clocks_or_rng": true,
    "bounded_runtime_cost": true,
    "conservative_fallback": true
  },
  "fallback": {
    "output_value": false,
    "reason": "disable candidate window"
  }
}
```

Guard decisions include the chosen output, machine-readable evidence terms,
fallback status, and reason string. Regression, corpus drift, insufficient
samples, new errors, invalid documents, or failed proof obligations all disable
the fast path and return the configured fallback output.

### 7. NUMA Read-Command Profile (manifest.json + env.json)

Manual high-core profiling bundles use `manifest.json` and `env.json` when the
goal is not a before/after optimization gate, but an evidence map for future
read-command work. These bundles are intended for 64+ core / high-RAM hosts and
must make CPU placement explicit.

`env.json` records the host, binary, and corpus:

```json
{
  "schema_version": "br.numa-profile-env.v1",
  "generated_at": "2026-05-04T22:00:00Z",
  "git": {
    "revision": "git-revision",
    "runtime_code_dirty": false
  },
  "host": {
    "kernel": "Linux 6.x x86_64 GNU/Linux",
    "cpu_model": "AMD Ryzen Threadripper PRO 5995WX 64-Cores",
    "logical_cpus": 128,
    "cores_per_socket": 64,
    "threads_per_core": 2,
    "sockets": 1,
    "numa_nodes": 1,
    "memory_total_bytes": 536069881856,
    "memory_available_bytes": 462368509952
  },
  "binary": {
    "path": "/data/tmp/.../release/br",
    "version": "br 0.2.5",
    "sha256": "64-char-sha256",
    "build_profile": "release --no-default-features"
  },
  "workspace": {
    "path": "/data/tmp/br-large-profile",
    "issue_count": 12000,
    "jsonl_sha256": "64-char-sha256",
    "storage_path": ".beads/beads.db"
  },
  "captured_files": {
    "lscpu_text": "env/lscpu.txt",
    "lscpu_json": "env/lscpu.json",
    "numactl_hardware": "env/numactl-hardware.txt",
    "memory": "env/free-bytes.txt"
  }
}
```

`manifest.json` records the command matrix, placement modes, and evidence
decomposition:

```json
{
  "schema_version": "br.numa-read-command-profile.v1",
  "generated_at": "2026-05-04T22:00:00Z",
  "bead": "beads_rust-72yf.39",
  "purpose": "High-core/NUMA-aware read-command evidence bundle.",
  "environment_path": "env.json",
  "profile_modes": [
    {
      "name": "default_scheduler",
      "timing_summary_path": "timing/default-summary.json",
      "raw_hyperfine_path": "timing/hyperfine-default.json"
    },
    {
      "name": "pinned_cpu0",
      "timing_summary_path": "timing/pinned-cpu0-summary.json",
      "raw_hyperfine_path": "timing/hyperfine-pinned-cpu0.json"
    },
    {
      "name": "cross_numa",
      "status": "unavailable_on_pilot_host",
      "reason": "numactl reported one NUMA node"
    }
  ],
  "commands": [
    {
      "label": "list_json_limit100",
      "args": ["list", "--json", "--limit", "100"],
      "stdout_path": "commands/list_json_limit100.json",
      "stderr_path": "commands/list_json_limit100.stderr",
      "syscall_summary_path": "syscalls/list_json_limit100.strace.txt"
    }
  ],
  "golden": {
    "checksums_path": "golden/command-output-sha256.txt"
  },
  "decomposition": {
    "queueing_lock": "lock-wait or contention evidence path",
    "service_cpu": "timing user/system split and pinned/default delta",
    "io_page_reads": "strace pread64/open/stat counters",
    "serialization_output": "stdout byte counts and golden outputs"
  },
  "notes_path": "notes.md"
}
```

Required command labels for the standard read profile are:

- `list_json_limit100`
- `ready_json_limit100`
- `scheduler_json_candidate100`
- `search_agent_json_limit100`
- `stats_no_activity_json`
- `label_list_all_json`

Cross-NUMA mode is required on hosts where `numactl --hardware` reports two or
more nodes. Single-node hosts must keep the `cross_numa` profile entry with
`status: "unavailable_on_pilot_host"` and preserve the raw `numactl` output.

### 8. Swarm Capacity Report (report.json + report.md)

Capacity-planning bundles translate perf evidence into operator-facing
concurrency bands. They do not claim a universal maximum. They record the
assumptions, measured command mix, uncertainty, and invalidation triggers used
to recommend green/yellow/red agent-count ranges.

```json
{
  "schema_version": "br.swarm-capacity-report.v1",
  "generated_at": "2026-05-04T22:30:00Z",
  "bead": "beads_rust-72yf.41",
  "source_evidence": {
    "numa_profile": "tests/artifacts/perf/.../manifest.json",
    "drift_guard_schema": "br.sequential-drift-guard.v1",
    "count_path": "inputs/count.json",
    "sync_status_path": "inputs/sync-status.json",
    "doctor_path": "inputs/doctor.json"
  },
  "workspace": {
    "issue_count": 12000,
    "dirty_count": 0,
    "doctor_ok": true
  },
  "hardware": {
    "logical_cpus": 128,
    "physical_cores": 64,
    "memory_total_bytes": 536069881856,
    "numa_nodes": 1
  },
  "model": {
    "weighted_p95_ms": 162,
    "assumed_commands_per_agent_per_second": 1.0,
    "p95_latency_budget_ms": 500,
    "uncertainty": "medium for read-heavy workloads; high for write-heavy workloads"
  },
  "recommendations": {
    "high_core_host": [
      {"band": "green", "agents": "1-64"},
      {"band": "yellow", "agents": "65-128"},
      {"band": "red", "agents": ">128"}
    ],
    "laptop_or_small_vm": [
      {"band": "green", "agents": "1-4"},
      {"band": "yellow", "agents": "5-8"},
      {"band": "red", "agents": ">8"}
    ]
  },
  "invalidation": {
    "rerun_when": ["dirty_count is nonzero", "weighted p95 regresses by >5%"],
    "protected_by": "SequentialDriftGuard"
  }
}
```

Each bundle must also include a human-readable `report.md`, copied input
evidence under `inputs/`, and `golden/report-sha256.txt` for snapshot-style
format checks.

## Directory Structure

```
target/test-artifacts/
└── <suite>/
    └── <test>/
        ├── events.jsonl          # Command and snapshot events
        ├── summary.json          # Test result summary
        ├── 0001_init.stdout      # Captured stdout
        ├── 0001_init.stderr      # Captured stderr (if non-empty)
        ├── before.snapshot.json  # File tree snapshot
        └── after.snapshot.json   # File tree snapshot

target/perf-artifacts/
└── <startup-matrix-run>/
    ├── startup-matrix-manifest.json
    ├── logs/
    ├── timing/
    ├── syscalls/
    ├── rss/
    └── raw/

target/perf-artifacts/
└── <perf-evidence-run>/
    ├── perf-evidence-manifest.json
    ├── baseline/
    ├── golden/
    ├── proof/
    ├── timing/
    ├── syscalls/
    ├── io/
    ├── rss/
    └── raw/

tests/artifacts/perf/
└── <numa-read-command-profile>/
    ├── manifest.json
    ├── env.json
    ├── env/
    ├── commands/
    ├── golden/
    ├── timing/
    ├── syscalls/
    └── raw/

tests/artifacts/perf/
└── <swarm-capacity-report>/
    ├── report.json
    ├── report.md
    ├── inputs/
    └── golden/
```

## Validation Rules

### Required Invariants

1. **Timestamp format**: Must be valid RFC3339 with timezone
2. **Event type**: Must be exactly `"command"` or `"snapshot"`
3. **Args array**: Must be array of strings, never null
4. **Path safety**: All paths must be relative (no `..` traversal)
5. **Exit codes**: Must be integer in range [-128, 255]
6. **Size values**: Must be non-negative integers
7. **Startup state coverage**: Startup matrix manifests must include every
   required startup state exactly once
8. **Aggregation evidence**: Partial or failed startup matrix aggregation must
   preserve raw evidence references
9. **Performance evidence freshness**: `valid_until`, when present, must be in
   the future
10. **Performance gate policy**: Enforcing mode requires a baseline manifest and
    a passing comparison; advisory mode may warn without blocking
11. **Golden hashes**: Performance evidence stdout/stderr hashes must be
    lowercase SHA-256 hex digests
12. **Timing order**: Performance evidence timing must satisfy
    `min <= p50 <= p95 <= p99 <= max`
13. **NUMA profile coverage**: NUMA read-command profiles must include the six
    standard command labels, default-scheduler timing, pinned-core timing,
    syscall summaries, golden stdout/stderr checksums, and a cross-NUMA profile
    entry that is either populated or explicitly marked unavailable.
14. **Drift guard fail-closed behavior**: Sequential drift guards must disable
    the fast path when proof obligations fail, corpus hashes drift, sample counts
    are below budget, candidate errors increase beyond budget, or p95 regression
    exceeds the configured percentage.
15. **Capacity report uncertainty**: Capacity reports must use bands with
    assumptions and invalidation rules, not a single unqualified maximum.

### Cross-Platform Normalization

1. **Line endings**: All text outputs normalized to `\n`
2. **Paths**: Forward slashes `/` on all platforms
3. **Timestamps**: Always UTC with `Z` suffix or offset

## Programmatic Validation

Use the `ArtifactValidator` in tests:

```rust
use beads_rust::test_utils::ArtifactValidator;

let validator = ArtifactValidator::new();
validator.validate_events_file("target/test-artifacts/e2e/test/events.jsonl")?;
validator.validate_snapshot_file("target/test-artifacts/e2e/test/before.snapshot.json")?;
validator.validate_summary_file("target/test-artifacts/e2e/test/summary.json")?;
validator.validate_startup_matrix_bundle_dir("target/perf-artifacts/startup-matrix-smoke-...")?;
validator.validate_perf_evidence_bundle_dir("target/perf-artifacts/perf-evidence-smoke-...")?;
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `HARNESS_ARTIFACTS` | `0` | Set to `1` to enable artifact logging |
| `HARNESS_PRESERVE_SUCCESS` | `0` | Set to `1` to keep all artifacts on success |

## Example: Valid events.jsonl

```jsonl
{"timestamp":"2026-01-17T12:34:56.000Z","event_type":"command","label":"init","binary":"br","args":["init"],"cwd":"/tmp/test","exit_code":0,"success":true,"duration_ms":42,"stdout_len":64,"stderr_len":0,"stdout_path":"0001_init.stdout","stderr_path":null,"snapshot_path":null}
{"timestamp":"2026-01-17T12:34:56.100Z","event_type":"snapshot","label":"after_init","binary":"","args":[],"cwd":"/tmp/test","exit_code":0,"success":true,"duration_ms":0,"stdout_len":0,"stderr_len":0,"stdout_path":null,"stderr_path":null,"snapshot_path":"after_init.snapshot.json"}
{"timestamp":"2026-01-17T12:34:56.200Z","event_type":"command","label":"create","binary":"br","args":["create","--title","Test issue"],"cwd":"/tmp/test","exit_code":0,"success":true,"duration_ms":15,"stdout_len":32,"stderr_len":0,"stdout_path":"0002_create.stdout","stderr_path":null,"snapshot_path":null}
```

## Report Generation

### Generating Human-Friendly Reports

The artifact report indexer generates HTML and Markdown reports from test artifacts
for faster triage of test failures.

**Task: beads_rust-x7on**

#### Quick Start

```bash
# 1. Run tests with artifacts enabled
HARNESS_ARTIFACTS=1 cargo test e2e_sync

# 2. Generate reports
./scripts/generate-report.sh

# 3. Open the report
open target/reports/report.html
```

#### Manual Report Generation

```bash
REPORT_ARTIFACTS_DIR=target/test-artifacts \
REPORT_OUTPUT_DIR=target/reports \
cargo test --test e2e_report_generation -- --nocapture --ignored generate_and_save_report
```

#### Programmatic Usage

```rust
use common::report_indexer::{ArtifactIndexer, write_reports};

// Create indexer
let indexer = ArtifactIndexer::new("target/test-artifacts");

// Generate report
let report = indexer.generate_report()?;

// Access report data
println!("Total tests: {}", report.total_tests);
println!("Pass rate: {:.1}%", report.pass_rate());

// Write HTML and Markdown files
let (md_path, html_path) = write_reports(&report, "target/reports")?;
```

#### Report Contents

The generated reports include:

- **Summary statistics**: Total tests, pass/fail counts, duration
- **Suite breakdown**: Per-suite results with test tables
- **Failed test details**: Failure reasons, failed commands, artifact links
- **Slowest tests**: Top 10 slowest tests for performance analysis

#### Configuration Options

```rust
use common::report_indexer::{ArtifactIndexer, IndexerConfig};

let config = IndexerConfig {
    artifact_root: PathBuf::from("target/test-artifacts"),
    failures_only: true,   // Only include failed tests
    max_tests: 100,        // Limit tests per suite (0 = unlimited)
    include_commands: true, // Include command details
    include_snapshots: true, // Include snapshot metrics
};

let indexer = ArtifactIndexer::with_config(config);
```

## References

- [E2E_COVERAGE_MATRIX.md](E2E_COVERAGE_MATRIX.md) - Test coverage mapping
- [TROUBLESHOOTING.md](TROUBLESHOOTING.md) - Error codes and JSON shapes
- [tests/common/harness.rs](../tests/common/harness.rs) - Harness implementation
- [tests/common/report_indexer.rs](../tests/common/report_indexer.rs) - Report indexer implementation

---

*Updated: 2026-01-18*
*Tasks: beads_rust-r23m, beads_rust-x7on*
*Agents: SilentFalcon (opus-4.5), Opus-C (opus-4.5)*
