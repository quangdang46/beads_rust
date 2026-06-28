# Scheduler Refill Probe

Baseline binary: `/data/tmp/br_72yf33_count_candidate_br`

Candidate binary: `/data/tmp/br_72yf34_scheduler_refill_candidate_br`

Workspace: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Change tested: avoid reloading the full ready set for scheduler candidate caps unless computed external blockers actually intersect the limited candidate prefix.

Behavior proof: before/after scheduler JSON outputs in this directory match after normalizing `generated_at` for default `scheduler --json`, `scheduler --candidate-limit 32 --json`, and `scheduler --candidate-limit 128 --json`.

Timing summary from `hyperfine.json`:

| Command | Before | After | Result |
| --- | ---: | ---: | --- |
| `scheduler --json` | 241.3 ms | 243.3 ms | flat |
| `scheduler --candidate-limit 128 --json` | 369.8 ms | 366.8 ms | flat |
| `scheduler --candidate-limit 64 --json` | 274.8 ms | 273.3 ms | flat |
| `scheduler --candidate-limit 32 --json` | 224.1 ms | 223.1 ms | flat |

Decision: keep the artifact as a measured no-go for this large matrix. The code landed independently in `10cdab87`, but this artifact should not be counted as a material matrix performance win.
