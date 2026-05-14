# Dependency Upgrade Log

**Date:** 2026-05-14 | **Project:** beads_rust | **Language:** Rust

## Summary

- **Updated:** 6 dependency families | **Skipped:** 0 | **Failed:** 0 | **Blocked:** 0

## Discovery

- Manifest: `Cargo.toml`
- Lock file: `Cargo.lock`
- Outdated direct dependencies from `cargo outdated --root-deps-only`: `assert_cmd`, `clap_complete`, `signal-hook`, `tru`.
- Sibling `/data/projects` dependencies checked: `frankensqlite` (`fsqlite*`), `toon_rust` (`tru`), `rich_rust`, and `fastmcp_rust`.
- Final `cargo outdated --root-deps-only` reports all direct dependencies up to date.

## Updates

### Audit warning remediation for v0.2.9

- **Issue:** `cargo audit` reported advisory warnings for `serde_yml`/`libyml`, `rand 0.8.5`, and `syntect` transitive crates pulled in through `rich_rust/full`.
- **Migration:** Repointed the local `serde_yml` crate alias to the maintained `serde_norway` package, updated the lockfile to patched `rand 0.8.6`, and stopped enabling `rich_rust`'s `syntax` feature because `br` does not use its exported syntax helper in command flows.
- **Tests:** Full all-features release-preparation suite passed after this remediation.

### clap_complete: 4.5.66 -> 4.6.5

- **Breaking:** None found for this project usage. This is a `clap` 4.x completion crate patch/minor-compatible bump; existing `unstable-dynamic` feature remains available.
- **Migration:** Manifest version only.
- **Tests:** `cargo test --lib --all-features` via RCH passed: 2157 passed, 0 failed, 7 ignored.

### Build metadata warnings: vergen git defaults

- **Issue:** RCH and package-style builds can have no usable `.git` metadata, causing `vergen-gix` to emit `VERGEN_GIT_* set to default` build warnings.
- **Migration:** Only emit git build metadata when `.git/HEAD` exists and a read-only `git rev-parse --is-inside-work-tree` probe confirms a usable work tree; package/non-git builds still emit build timestamp, target triple, and rustc version without warning.
- **Tests:** `cargo test --lib --all-features` via RCH passed without the previous `VERGEN_GIT_* set to default` warnings: 2157 passed, 0 failed, 7 ignored.

### assert_cmd: 2.2.1 -> 2.2.2

- **Breaking:** None found. Patch release in the same 2.x testing helper line.
- **Migration:** Manifest version only.
- **Tests:** `cargo test --lib --all-features` via RCH passed: 2157 passed, 0 failed, 7 ignored.

### signal-hook: 0.3.x -> 0.4.4

- **Breaking:** None found for this project usage. The direct dependency now targets the current 0.4 line; `crossterm` still retains its own compatible 0.3 transitive dependency.
- **Migration:** Manifest version only; no code changes required.
- **Tests:** `cargo test --lib --all-features` via RCH passed: 2157 passed, 0 failed, 7 ignored.

### tru (`toon_rust`): 0.2.2 -> 0.2.3

- **Breaking:** None found for current TOON formatting usage.
- **Migration:** Manifest and lockfile update.
- **Tests:** `cargo test --lib --all-features` via RCH passed: 2157 passed, 0 failed, 7 ignored.

### fsqlite stack: 0.1.2/0.1.3 -> latest published local stack

- **Updated:** `fsqlite`, `fsqlite-types`, `fsqlite-error`, `fsqlite-core`, `fsqlite-func`, `fsqlite-vdbe`, `fsqlite-pager`, `fsqlite-parser`, `fsqlite-planner`, `fsqlite-wal`, `fsqlite-btree`, `fsqlite-ast`, `fsqlite-mvcc`, and `fsqlite-observability` to `0.1.3`; `fsqlite-vfs` to `0.1.4`.
- **Breaking:** The newer WAL/pager behavior exposed noisy transient tail-read diagnostics and lock-timeout semantics in read-only fast-open tests.
- **Migration:** Respect explicit `--lock-timeout` by using the conservative storage open path, downgrade expected transient WAL tail-read fallback logs, and keep default debug logs focused on `beads_rust` rather than fsqlite internals.
- **Tests:** Full all-features release-preparation suite passed.

## Blockers / Needs Attention

### `fastmcp_rust` local version published and consumed

- `frankensqlite` is aligned with the latest published local stack used by `beads_rust`.
- `fastmcp_rust` local workspace `0.3.1` has been published to crates.io across the FastMCP crate family.
- `beads_rust` now depends on `fastmcp-rust = 0.3.1`, satisfying the latest-local-library requirement while keeping crates.io publication viable.

## Validation

- `cargo outdated --root-deps-only` reports all direct dependencies up to date.
- `cargo check --all-targets --all-features` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test --all-features --no-fail-fast` passed, including doctests.
- `cargo audit` passed with no advisory warnings after the v0.2.9 remediation.
- `cargo publish --dry-run --locked` passed for `beads_rust v0.2.9`.

## Release Status

- Prepared `beads_rust v0.2.9`.
- `v0.2.9` supersedes `v0.2.8`; `v0.2.8` was already published to crates.io before the Windows release-build fix.
