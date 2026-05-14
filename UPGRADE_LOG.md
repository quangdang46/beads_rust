# Dependency Upgrade Log

**Date:** 2026-05-14 | **Project:** beads_rust | **Language:** Rust

## Summary

- **Updated:** 4 | **Skipped:** 0 | **Failed:** 0 | **Blocked:** 2

## Discovery

- Manifest: `Cargo.toml`
- Lock file: `Cargo.lock`
- Outdated direct dependencies from `cargo outdated --root-deps-only`: `assert_cmd`, `clap_complete`, `signal-hook`, `tru`.
- Sibling `/data/projects` dependencies checked: `frankensqlite` (`fsqlite*`), `toon_rust` (`tru`), `rich_rust`, and `fastmcp_rust`.

## Updates

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

## Blockers / Needs Attention

### Local `/data/projects` dependency versions ahead of crates.io

- `frankensqlite` local manifests are at `fsqlite* = 0.1.3` and `fsqlite-vfs = 0.1.4`, but crates.io currently reports `fsqlite* = 0.1.2` and `fsqlite-vfs = 0.1.3`.
- `fastmcp_rust` local workspace is `0.3.1`, while crates.io currently reports `fastmcp-rust = 0.3.0`.
- `beads_rust` cannot publish to crates.io with those local-only versions until the upstream crates are published or the release intentionally stays on the latest published registry versions.

## Validation

- Incremental all-features library tests via RCH pass after each dependency update.
- Full release-preparation validation pending.
