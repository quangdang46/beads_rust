//! Build script for `beads_rust`.
//!
//! Uses vergen-gix to embed build information into the binary.

use std::path::Path;
use std::process::Command;
use vergen_gix::{BuildBuilder, CargoBuilder, Emitter, GixBuilder, RustcBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let build = BuildBuilder::default().build_timestamp(true).build()?;
    let cargo = CargoBuilder::default().target_triple(true).build()?;
    let rustc = RustcBuilder::default().semver(true).build()?;

    let mut emitter = Emitter::default();
    emitter
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&rustc)?;

    if Path::new(".git/HEAD").is_file() && is_inside_git_work_tree() {
        let gix = GixBuilder::default()
            .branch(true)
            .sha(true)
            .commit_timestamp(true)
            .dirty(true)
            .build()?;
        emitter.add_instructions(&gix)?;
    }

    emitter.emit()?;

    Ok(())
}

fn is_inside_git_work_tree() -> bool {
    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output();

    matches!(
        output,
        Ok(output) if output.status.success() && output.stdout == b"true\n"
    )
}
