# Dependency Upgrade Log

**Date:** 2026-05-14 | **Project:** beads_rust | **Language:** Rust

## Summary

- **Updated:** 0 | **Skipped:** 0 | **Failed:** 0 | **Blocked:** 0

## Discovery

- Manifest: `Cargo.toml`
- Lock file: `Cargo.lock`
- Outdated direct dependencies from `cargo outdated --root-deps-only`: `assert_cmd`, `clap_complete`, `signal-hook`, `tru`.
- Sibling `/data/projects` dependencies checked: `frankensqlite` (`fsqlite*`), `toon_rust` (`tru`), `rich_rust`, and `fastmcp_rust`.

## Updates

### clap_complete: 4.5.66 -> 4.6.5

- **Breaking:** None found for this project usage. This is a `clap` 4.x completion crate patch/minor-compatible bump; existing `unstable-dynamic` feature remains available.
- **Migration:** Manifest version only.
- **Tests:** Pending.

## Blockers / Needs Attention

### Local `/data/projects` dependency versions ahead of crates.io

- `frankensqlite` local manifests are at `fsqlite* = 0.1.3` and `fsqlite-vfs = 0.1.4`, but crates.io currently reports `fsqlite* = 0.1.2` and `fsqlite-vfs = 0.1.3`.
- `fastmcp_rust` local workspace is `0.3.1`, while crates.io currently reports `fastmcp-rust = 0.3.0`.
- `beads_rust` cannot publish to crates.io with those local-only versions until the upstream crates are published or the release intentionally stays on the latest published registry versions.

## Validation

- Pending.
