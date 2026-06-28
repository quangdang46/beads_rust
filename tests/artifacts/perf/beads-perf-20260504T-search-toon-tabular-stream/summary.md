# Search TOON tabular stream

Dataset: `/tmp/br-blocked-projection-real2-tTf4Nx`

Command:

```bash
br search blocked --limit 0 --format toon > /dev/null
```

Baseline binary: `/data/tmp/br-list-text-priority-window-candidate`

Candidate binary: `/data/tmp/br-search-toon-tabular-stream-candidate-20260504`

## Result

Hyperfine, 5 warmups and 30 timed runs:

| Binary | Mean | Stddev | User | System |
| --- | ---: | ---: | ---: | ---: |
| Baseline | 509.1 ms | 4.6 ms | 388.5 ms | 119.7 ms |
| Candidate | 451.6 ms | 5.0 ms | 340.7 ms | 110.4 ms |

Candidate ran `1.13x +/- 0.02x` faster.

Spot `/usr/bin/time -v` max RSS:

| Binary | Max RSS |
| --- | ---: |
| Baseline | 197,700 KB |
| Candidate | 176,372 KB |

## Parity

Default TOON stdout matched byte-for-byte:

```text
19284185 bytes
sha256 2bbff8d0d1e3579768d40889361d6d494324128bd2f52a653c282d9af4b3d890
```

`TOON_STATS=1` stayed on the existing encoder and matched stdout and stderr
byte-for-byte:

```text
[stats] JSON: 21276002 chars, TOON: 19284184 chars (9% savings)
```

Empty search output also matched:

```text
[0]:
```
