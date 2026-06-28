| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/data/tmp/br_d824_72yf26_target/release/br list --limit 50 --json --label lane-00 >/dev/null` | 240.8 ± 1.8 | 237.9 | 242.6 | 1.10 ± 0.02 |
| `/data/tmp/br_72yf26_local_target/release/br list --limit 50 --json --label lane-00 >/dev/null` | 236.8 ± 7.3 | 224.8 | 241.7 | 1.08 ± 0.04 |
| `/data/tmp/br_d824_72yf26_target/release/br search payload --json --label lane-00 >/dev/null` | 221.1 ± 3.6 | 216.0 | 224.0 | 1.01 ± 0.02 |
| `/data/tmp/br_72yf26_local_target/release/br search payload --json --label lane-00 >/dev/null` | 219.3 ± 2.9 | 214.8 | 222.3 | 1.00 |
