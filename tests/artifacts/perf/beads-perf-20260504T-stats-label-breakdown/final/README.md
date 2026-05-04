# stats --by-label label breakdown optimization

Bead: `beads_rust-72yf.28`

Dataset: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Control binary: `/data/tmp/br_72yf27_control_br` (`b20e2609`)

Retained candidate binary: `/data/tmp/br_72yf28_candidate2_br`

## Retained result

Command: `stats --by-label --json`

| Binary | Mean |
| --- | ---: |
| control | 204.7 ms +/- 3.6 ms |
| candidate | 175.7 ms +/- 2.7 ms |

Speedup: 1.16x.

Guard command: `stats --json`

| Binary | Mean |
| --- | ---: |
| control | 133.5 ms +/- 2.1 ms |
| candidate | 133.4 ms +/- 2.8 ms |

The retained implementation scans unordered label pairs and filters them against
the already-loaded stats issue rows. This avoids building the export-oriented
`issue_id -> Vec<label>` map and avoids a slower SQL grouped join.

## Behavior proof

Stable JSON outputs were captured with `--no-activity` to avoid volatile git
activity fields:

| Output | SHA-256 |
| --- | --- |
| `control-stats-by-label-no-activity.json` | `b0f7c5af8e3b66320fa974dc94114658800b20a17285c255988d8ed05f24cdee` |
| `candidate-stats-by-label-no-activity.json` | `b0f7c5af8e3b66320fa974dc94114658800b20a17285c255988d8ed05f24cdee` |
| `control-stats-no-activity.json` | `5e434a2120c37b2b44dc2fe7659adcb0b70702f689336b6042771679d7c1f571` |
| `candidate-stats-no-activity.json` | `5e434a2120c37b2b44dc2fe7659adcb0b70702f689336b6042771679d7c1f571` |

Both control/candidate output pairs also passed `cmp`.

## Rejected candidate

An earlier candidate replaced stats label breakdown with
`count_labels_with_filters()` using broad filters. It regressed
`stats --by-label --json` from 201.7 ms +/- 5.4 ms to 338.4 ms +/- 10.3 ms, so
the SQL grouped join path was not retained.
