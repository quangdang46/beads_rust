# Audit: bd→br Fixture Migration Sweep (2026-05-09)

**Operator:** audit-2026-05-09
**Bead:** beads_rust-6plg
**Trigger:** Sibling bead `beads_rust-uelt` discovered `tests/repro_cache_crash.rs` used `bd-a/b/c` test fixture IDs in a `br` codebase. This bead audits the entire project for stale `bd-` references that need migration.

---

## ⚠️ CORRECTION (2026-09-30) — read before citing any number below

Re-verified on 2026-09-30 against git history. Everything below this line is preserved **verbatim** as the record of what was believed on 2026-05-09. Its TL;DR verdict survives; its evidence, its scale, and its central numbers do not. Do not quote the figures.

### The counts are wrong

| Audit claim | Actual | How the actual was measured |
|---|---:|---|
| Raw count: **76,291** | **~3,229 lines / 3,326 occurrences** | Re-derived at the audit's own commit `7716952b`; ~23–25× overstatement |
| **~72,000** hits in `tests/artifacts/perf/` | **4** | 2 at `55594399`, 2 at HEAD. Both are in `beads-perf-20260503T-dep-tree-local-traversal/dep-tree-single-node-{old-vs-local,text-old-vs-local}.json` |
| perf tree is **6 files** | **335 files / 57 immediate subdirectories** (65 counting nested `final/`, `baseline/` variant dirs), 8 of them `.toon` | 335 blob paths under `tests/artifacts/perf/` at `55594399` |
| perf tree: **frozen baseline; must NOT be touched** | Modified 2026-07-04 (`3ba7d039`): 51 files changed, +520,577 / −843, **0 added, 0 deleted**, 21.0 MB → 24.0 MB | `git show --numstat 3ba7d039 -- tests/artifacts/perf/` |
| `src/` doc comments **~600** | **~80** pure-comment lines, of which **~23** are rustdoc | ~15× overstatement |
| `src/` `#[cfg(test)]` fixtures **~1,500** | **~2,300–2,460** | `src/` carries 2,530 `bd-` lines; 80 are comments, so ~2,450 are not |
| After excluding perf: **2,551** | **~3,225** | ~26% **increase** — the opposite direction from the audit's framing |
| `docs/CLI_REFERENCE.md`: **12** hits, "illustrating cross-tool prefix tolerance" | **13** hits, **no prose supporting that reading** | see below |
| **User-facing legacy `bd <command>` syntax \| 0 \| N/A — all migrated** | **Never measured** | see below |

### The `0 user-facing bd <command>` row was not evidence

The stated method is `rg 'bd-[a-z0-9]'` — a **hyphen** pattern. It structurally cannot match the **space** form `bd <command>`, so the table's final row was never tested by the audit's own instrument. "N/A — all migrated" was asserted, not observed.

Run the space form over the audit's declared scope and it returns ~137 hits, all intentional: 79 in `docs/porting/EXISTING_BEADS_STRUCTURE_AND_ARCHITECTURE.md`, 21 in `docs/BD_VS_BR.md`, 10 in the port plan, 1 in `docs/SYNC_SAFETY.md:237`. **`AGENTS.md` and `README.md` return zero.** So the *conclusion* holds for the scope the audit declared — but the row was not the evidence for it.

### The `docs/CLI_REFERENCE.md` rationale was invented

The 13 `bd-abc123` occurrences (lines 359, 362, 365, 406, 409, 412, 415, 440, 443, 446, 449, 2075, 2104) all sit inside fenced code blocks or JSON samples. There is no prose anywhere in the file describing cross-tool prefix tolerance. The audit attributed a motivation the file does not state.

### The ~72,000 did not come from this repository

Four occurrences exist in the perf tree, not 72,000, and no tracked tree at any commit holds a 6-file perf directory. That figure most likely came from untracked or gitignored working-directory files on the operator's machine, which would also explain why it has never been reproducible since.

This is the load-bearing error. The perf-artifact category was **94% of the raw count** (~72,000 of 76,291) and the stated basis for the recommendation not to lint `bd-`. That recommendation may still be right on the merits — the audit's *other* three categories really are legitimate — but as written it rests on a category that did not exist as described, and the measured weighting inverts: excluding perf, the count **grew** ~26% in the four months since.

### The `src/` classification had no bucket for user-facing strings

The audit sorted every `src/` hit into exactly two bins: doc comments and `#[cfg(test)]` fixtures. A user-visible error message is neither, and would have been caught by the audit's own pattern. One is still present:

```rust
// src/storage/sqlite.rs:3423
return Err(BeadsError::validation(
    "new_id",
    "must be prefix-suffix format (e.g., bd-dolt, gt-auth)",
));
```

A `br` user who mistypes an ID is told to imitate `bd-dolt`.

### Scope

The path list was `tests/ src/ docs/ AGENTS.md README.md`. That excludes `web/`, `install.sh`, `install.ps1`, and everything else. A later sweep on 2026-09-30 found real user-facing regressions in exactly those excluded areas, which is consistent with the TL;DR's conclusion being **true within a scope narrower than the repository** rather than false.

### Does `UPSTREAM_GAP_AUDIT_2026_09_25.md` supersede this?

No. That audit is scoped to functional parity — LOCAL `br` vs `Dicklesworthstone/beads_rust` vs `gastownhall/beads` across 12 capability domains. It contains no finding about `bd`→`br` naming, re-measures none of these counts, and never references this document. Its `testing-parity-docs` domain does flag stale documentation as a class, which is adjacent but distinct. The two do not overlap and neither replaces the other; between them they are the repo's only two dated doc-accuracy records, and only this one covers naming.

### What still holds

The TL;DR's core claim — the migration is complete in **user-facing surfaces** — survives re-verification: `AGENTS.md` and `README.md` contain no stale `bd <command>`, and the ~137 space-form hits across `docs/` are all deliberate (historical port plans, the bd-vs-br matrix, one incident citation). What does not survive is the evidence offered for it, and the recommendation's stated basis.

---

## TL;DR

**Migration is essentially complete.** The bd→br rename is done in user-facing surfaces (AGENTS.md, README.md, the failing test). Remaining `bd-` references are concentrated in:

1. **Perf artifacts** (~72,000 hits, 6 files in `tests/artifacts/perf/`) — frozen baseline test data; must NOT be touched.
2. **Illustrative-example test fixtures in `src/` `#[cfg(test)]` blocks** — internal-test fixtures using `bd-`-prefix as a generic placeholder; these don't appear in user-facing surfaces.
3. **Doc comments in source code** — illustrative IDs like `bd-epic.1`, `bd-abc.2` in rustdoc explaining behavior.
4. **`docs/CLI_REFERENCE.md`** — uses `bd-abc123` in `br show bd-abc123` examples to illustrate that `br` accepts any prefix (cross-tool tolerance).
5. **`docs/porting/PLAN_TO_PORT_BEADS_WITH_SQLITE_AND_ISSUES_JSONL_TO_RUST.md`** — historical document about the Go-to-Rust port; intentionally preserves the original `bd <command>` references.
6. **`docs/SYNC_SAFETY.md` line 237** — single reference to `bd sync` in a historical incident citation.

## Methodology

```bash
rg --no-heading -n 'bd-[a-z0-9]' tests/ src/ docs/ AGENTS.md README.md
```

**Raw count:** 76,291 hits across the project.
**After filtering `tests/artifacts/perf/`:** 2,551 hits.

## Per-category classification

| Category | Hit count | Action |
|----------|----------:|--------|
| **Perf artifacts** (`tests/artifacts/perf/*.toon`, 6 files × ~12,000 hits each) | ~72,000 | KEEP — frozen baseline test data |
| **`src/` doc comments** (e.g., `/// status annotations like "bd-123:open"`) | ~600 | KEEP — illustrative comments showing prefix-tolerance |
| **`src/` `#[cfg(test)]` fixtures** (e.g., `"bd-ready-summary"`, `"bd-1"`) | ~1500 | KEEP — internal test fixtures; not user-visible |
| **`docs/porting/`** (port-plan documents) | ~120 | KEEP — historical Go→Rust port docs preserve bd-syntax intentionally |
| **`docs/CLI_REFERENCE.md`** | 12 (`br show bd-abc123` examples) | KEEP — illustrating cross-tool prefix tolerance |
| **`docs/SYNC_SAFETY.md` line 237** | 1 | KEEP — historical incident citation about `bd sync` |
| **AGENTS.md, README.md** | 1 (mention of `bd-to-br-migration` skill) | KEEP — meta-reference to the skill |
| **User-facing legacy `bd <command>` syntax** | 0 | N/A — all migrated |

## Conclusion

The migration is functionally complete. The `bd-` references that remain in the codebase fall into three legitimate categories:

1. **Cross-tool tolerance**: br accepts `bd-`-prefixed IDs as a deliberate compatibility feature; tests verify this and docs illustrate it.
2. **Frozen baselines**: perf-test artifacts are immutable historical snapshots.
3. **Historical documentation**: port-plan docs and incident citations intentionally preserve the original syntax.

`tests/repro_cache_crash.rs` was the one true migration target, fixed by sibling bead `beads_rust-uelt`.

## Per the bd-to-br-migration skill

The skill at `~/.claude/skills/bd-to-br-migration/` defines the migration as:
- Section headers: `bd (beads)` → `br (beads_rust)` ✓ (done)
- Commands: `bd <X>` → `br <X>` ✓ (done in user-facing surfaces)
- Sync command: `bd sync` → `br sync --flush-only` ✓ (no remaining `bd sync` in user-facing docs; the 1 citation in SYNC_SAFETY.md is historical context)
- Issue IDs: `bd-###` → `br-###` ✓ (in test fixtures; the remaining bd- usage is illustrative)
- Daemon references / auto-commit / hook installation / RPC mode: ✓ (none in src/ or user-facing docs)

## Recommendation: do NOT add a CI lint for `bd-` references

> **[Correction 2026-09-30]** The stated basis for this recommendation — the ~72,000-hit frozen-perf-baseline category — did not exist as described (see the correction section at the top). The recommendation may still be sound on its other three categories, but this argument does not support it.

A lint that fails on any `bd-` reference would be too aggressive — it would flag all the legitimate categories above. If a future regression is concerning (someone adds a NEW `bd <command>` example in user-facing docs), the better lint is targeted: grep for `\\bbd (sync|create|list|update|close|show|dep|ready|stats)\\b` in `docs/`, `AGENTS.md`, and `README.md`. That's the `bd-to-br-migration` skill's actual rule.
