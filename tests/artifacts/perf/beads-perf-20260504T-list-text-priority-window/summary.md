# list text priority window

Fixture: `/tmp/br-blocked-projection-real2-tTf4Nx`

Candidate: `/data/tmp/br-list-text-priority-window-candidate`

Baseline: `/data/tmp/br-list-text-limit-candidate`

Command:

```bash
br list --limit 20 --format text
```

Result:

- Baseline projected text list: `174.3 ms +/- 6.9 ms`
- Priority-window projected text list: `144.7 ms +/- 2.4 ms`
- Speedup: `1.20x +/- 0.05x`

Output proof:

- `list --limit 20 --format text`: byte-identical, `940` bytes, sha256 `a7bf0df4deece328e11d22cccd2699ce580c6d025dc85cd3fd2e25608be2ff3c`
- `list --limit 20 --offset 5 --format text`: byte-identical, `940` bytes, sha256 `bffe38736aeb11ae273d885c02e89ccc63718d69f6899f2677bf43e9a7e1a058`

Rejected probes:

- Full expression index on `COALESCE(priority, 2), created_at DESC, id`: byte-identical but flat at `173.5 ms +/- 3.8 ms` versus `173.0 ms +/- 5.1 ms`.
- Priority-window without the active visibility predicates: byte-identical on this fixture but not behavior-preserving in general; it only reached `1.25x`.
